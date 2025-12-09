// SPDX-License-Identifier: GPL-2.0
/*
 * magic_reboot - Emergency reboot via authenticated UDP packet
 *
 * This module listens for a specific 64-byte UDP packet and triggers
 * an immediate emergency reboot (SysRq-b) when received. Useful for
 * recovering wedged systems where userspace is unresponsive.
 *
 * Supports both IPv4 and IPv6.
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/init.h>
#include <linux/netfilter.h>
#include <linux/netfilter_ipv4.h>
#include <linux/netfilter_ipv6.h>
#include <linux/skbuff.h>
#include <linux/ip.h>
#include <linux/ipv6.h>
#include <linux/udp.h>
#include <linux/sysrq.h>
#include <linux/fs.h>
#include <linux/slab.h>

#define MAGIC_PACKET_SIZE 64
#define DEFAULT_PORT 999
#define DEFAULT_KEY_PATH "/run/agenix/magic-reboot.key"

MODULE_LICENSE("GPL");
MODULE_AUTHOR("Svein Ove Aas");
MODULE_DESCRIPTION("Emergency reboot via magic UDP packet");
MODULE_VERSION("0.1.0");

/* Module parameters */
static int port = DEFAULT_PORT;
module_param(port, int, 0444);
MODULE_PARM_DESC(port, "UDP port to listen on (default: 999)");

static char *key_path = DEFAULT_KEY_PATH;
module_param(key_path, charp, 0444);
MODULE_PARM_DESC(key_path, "Path to 64-byte magic packet file");

static int dryrun = 0;
module_param(dryrun, int, 0644);
MODULE_PARM_DESC(dryrun, "If 1, log matches but don't reboot (default: 0)");

static int debug = 0;
module_param(debug, int, 0644);
MODULE_PARM_DESC(debug, "If 1, log all received packets on the magic port (default: 0)");

/* Storage for the magic packet */
static u8 magic_packet[MAGIC_PACKET_SIZE];
static bool magic_loaded = false;

/* Netfilter hooks - one for IPv4, one for IPv6 */
static struct nf_hook_ops magic_nf_hook_v4;
static struct nf_hook_ops magic_nf_hook_v6;
static bool hook_v6_registered = false;

/*
 * Load the magic packet from the key file.
 * Returns 0 on success, negative error code on failure.
 */
static int load_magic_packet(void)
{
	struct file *f;
	loff_t pos = 0;
	ssize_t ret;

	f = filp_open(key_path, O_RDONLY, 0);
	if (IS_ERR(f)) {
		pr_err("magic_reboot: Failed to open key file %s: %ld\n",
		       key_path, PTR_ERR(f));
		return PTR_ERR(f);
	}

	ret = kernel_read(f, magic_packet, MAGIC_PACKET_SIZE, &pos);
	filp_close(f, NULL);

	if (ret != MAGIC_PACKET_SIZE) {
		pr_err("magic_reboot: Key file must be exactly %d bytes (got %zd)\n",
		       MAGIC_PACKET_SIZE, ret);
		return ret < 0 ? ret : -EINVAL;
	}

	magic_loaded = true;
	pr_info("magic_reboot: Loaded magic packet from %s\n", key_path);
	return 0;
}

/*
 * Check UDP payload and trigger reboot if it matches.
 * Common logic for both IPv4 and IPv6.
 */
static void check_and_reboot(struct udphdr *udph, const char *src_addr)
{
	unsigned char *payload;
	unsigned int payload_len;

	/* Calculate payload length */
	payload_len = ntohs(udph->len) - sizeof(struct udphdr);

	if (debug)
		pr_info("magic_reboot: Received %u byte UDP packet from %s on port %d\n",
			payload_len, src_addr, port);

	/* Check payload size */
	if (payload_len != MAGIC_PACKET_SIZE) {
		if (debug)
			pr_info("magic_reboot: Wrong size (expected %d, got %u)\n",
				MAGIC_PACKET_SIZE, payload_len);
		return;
	}

	payload = (unsigned char *)udph + sizeof(struct udphdr);

	/* Compare with magic packet */
	if (memcmp(payload, magic_packet, MAGIC_PACKET_SIZE) != 0) {
		if (debug)
			pr_info("magic_reboot: Packet content does not match magic key\n");
		return;
	}

	/* Match! */
	pr_emerg("magic_reboot: Received valid magic packet from %s\n", src_addr);

	if (dryrun) {
		pr_info("magic_reboot: DRYRUN mode - would trigger reboot now\n");
		return;
	}

	pr_emerg("magic_reboot: Triggering emergency reboot!\n");

	/* Trigger SysRq-b (immediate reboot) */
	handle_sysrq('b');
}

/*
 * Netfilter hook function for IPv4 packets.
 */
static unsigned int magic_reboot_hook_v4(void *priv,
					 struct sk_buff *skb,
					 const struct nf_hook_state *state)
{
	struct iphdr *iph;
	struct udphdr *udph;
	char src_addr[16];

	if (!magic_loaded)
		return NF_ACCEPT;

	/* Need at least IP header */
	if (!pskb_may_pull(skb, sizeof(struct iphdr)))
		return NF_ACCEPT;

	iph = ip_hdr(skb);

	/* Only process UDP */
	if (iph->protocol != IPPROTO_UDP)
		return NF_ACCEPT;

	/* Pull in UDP header */
	if (!pskb_may_pull(skb, iph->ihl * 4 + sizeof(struct udphdr)))
		return NF_ACCEPT;

	/* Re-fetch IP header after pskb_may_pull (may have reallocated) */
	iph = ip_hdr(skb);
	udph = (struct udphdr *)((unsigned char *)iph + iph->ihl * 4);

	/* Check destination port */
	if (ntohs(udph->dest) != port)
		return NF_ACCEPT;

	/* Pull in the full packet */
	if (!pskb_may_pull(skb, iph->ihl * 4 + ntohs(udph->len)))
		return NF_ACCEPT;

	/* Re-fetch headers after pskb_may_pull */
	iph = ip_hdr(skb);
	udph = (struct udphdr *)((unsigned char *)iph + iph->ihl * 4);

	snprintf(src_addr, sizeof(src_addr), "%pI4", &iph->saddr);
	check_and_reboot(udph, src_addr);

	return NF_ACCEPT;
}

/*
 * Netfilter hook function for IPv6 packets.
 */
static unsigned int magic_reboot_hook_v6(void *priv,
					 struct sk_buff *skb,
					 const struct nf_hook_state *state)
{
	struct ipv6hdr *ip6h;
	struct udphdr *udph;
	char src_addr[40];
	int offset;
	u8 nexthdr;
	__be16 frag_off;

	if (!magic_loaded)
		return NF_ACCEPT;

	/* Need at least IPv6 header */
	if (!pskb_may_pull(skb, sizeof(struct ipv6hdr)))
		return NF_ACCEPT;

	ip6h = ipv6_hdr(skb);

	/* Find UDP header, skipping extension headers */
	nexthdr = ip6h->nexthdr;
	offset = sizeof(struct ipv6hdr);

	offset = ipv6_skip_exthdr(skb, offset, &nexthdr, &frag_off);
	if (offset < 0)
		return NF_ACCEPT;

	/* Only process UDP */
	if (nexthdr != IPPROTO_UDP)
		return NF_ACCEPT;

	/* Pull in UDP header */
	if (!pskb_may_pull(skb, offset + sizeof(struct udphdr)))
		return NF_ACCEPT;

	/* Re-fetch IPv6 header after pskb_may_pull */
	ip6h = ipv6_hdr(skb);
	udph = (struct udphdr *)(skb->data + offset);

	/* Check destination port */
	if (ntohs(udph->dest) != port)
		return NF_ACCEPT;

	/* Pull in the full UDP packet */
	if (!pskb_may_pull(skb, offset + ntohs(udph->len)))
		return NF_ACCEPT;

	/* Re-fetch headers after pskb_may_pull */
	ip6h = ipv6_hdr(skb);
	udph = (struct udphdr *)(skb->data + offset);

	snprintf(src_addr, sizeof(src_addr), "%pI6c", &ip6h->saddr);
	check_and_reboot(udph, src_addr);

	return NF_ACCEPT;
}

static int __init magic_reboot_init(void)
{
	int ret;

	pr_info("magic_reboot: Initializing (port=%d, key=%s, dryrun=%d)\n",
		port, key_path, dryrun);

	/* Load the magic packet */
	ret = load_magic_packet();
	if (ret < 0) {
		pr_err("magic_reboot: Failed to load magic packet, module inactive\n");
		return ret;
	}

	/* Register IPv4 netfilter hook */
	magic_nf_hook_v4.hook = magic_reboot_hook_v4;
	magic_nf_hook_v4.hooknum = NF_INET_LOCAL_IN;
	magic_nf_hook_v4.pf = PF_INET;
	magic_nf_hook_v4.priority = NF_IP_PRI_FIRST;

	ret = nf_register_net_hook(&init_net, &magic_nf_hook_v4);
	if (ret < 0) {
		pr_err("magic_reboot: Failed to register IPv4 netfilter hook: %d\n", ret);
		return ret;
	}

	/* Register IPv6 netfilter hook */
	magic_nf_hook_v6.hook = magic_reboot_hook_v6;
	magic_nf_hook_v6.hooknum = NF_INET_LOCAL_IN;
	magic_nf_hook_v6.pf = PF_INET6;
	magic_nf_hook_v6.priority = NF_IP_PRI_FIRST;

	ret = nf_register_net_hook(&init_net, &magic_nf_hook_v6);
	if (ret < 0) {
		pr_warn("magic_reboot: Failed to register IPv6 netfilter hook: %d (IPv6 disabled)\n", ret);
		/* Continue without IPv6 - not fatal */
	} else {
		hook_v6_registered = true;
	}

	pr_info("magic_reboot: Module loaded, listening on UDP port %d (IPv4%s)\n",
		port, hook_v6_registered ? "+IPv6" : " only");
	return 0;
}

static void __exit magic_reboot_exit(void)
{
	nf_unregister_net_hook(&init_net, &magic_nf_hook_v4);
	if (hook_v6_registered)
		nf_unregister_net_hook(&init_net, &magic_nf_hook_v6);
	pr_info("magic_reboot: Module unloaded\n");
}

module_init(magic_reboot_init);
module_exit(magic_reboot_exit);
