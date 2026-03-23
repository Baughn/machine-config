/*
 * drm-atomic-log: LD_PRELOAD shim that intercepts DRM atomic modesetting
 * ioctls and serializes them to disk for debugging.
 *
 * Usage:
 *   LD_PRELOAD=/path/to/drm-atomic-log.so DRM_SHIM_LOG_DIR=/tmp/drm-atomic-log kwin_wayland
 *
 * Output: one log file per process at $DRM_SHIM_LOG_DIR/drm-atomic-<pid>.log
 */

#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

#include <drm/drm.h>
#include <drm/drm_mode.h>

/* ── Caches ─────────────────────────────────────────────────────────── */

#define MAX_PROPS   1024
#define MAX_BLOBS    256
#define MAX_OBJECTS  256

static struct {
    uint32_t prop_id;
    char     name[DRM_PROP_NAME_LEN]; /* 32 */
    uint32_t flags;
} prop_cache[MAX_PROPS];
static int prop_cache_count;

static struct {
    uint32_t blob_id;
    uint32_t length;
    uint8_t  data[256];
} blob_cache[MAX_BLOBS];
static int blob_cache_count;

static struct {
    uint32_t obj_id;
    uint32_t obj_type;
} obj_cache[MAX_OBJECTS];
static int obj_cache_count;

static FILE *log_fp;
static int atomic_seq;
static pthread_mutex_t shim_mutex = PTHREAD_MUTEX_INITIALIZER;

static int (*real_ioctl)(int fd, unsigned long request, ...) = NULL;

/* ── Helpers ────────────────────────────────────────────────────────── */

static void ensure_real_ioctl(void)
{
    if (!real_ioctl)
        real_ioctl = dlsym(RTLD_NEXT, "ioctl");
}

static const char *obj_type_str(uint32_t t)
{
    switch (t) {
    case DRM_MODE_OBJECT_CRTC:      return "CRTC";
    case DRM_MODE_OBJECT_CONNECTOR: return "Connector";
    case DRM_MODE_OBJECT_ENCODER:   return "Encoder";
    case DRM_MODE_OBJECT_MODE:      return "Mode";
    case DRM_MODE_OBJECT_PROPERTY:  return "Property";
    case DRM_MODE_OBJECT_FB:        return "FB";
    case DRM_MODE_OBJECT_BLOB:      return "Blob";
    case DRM_MODE_OBJECT_PLANE:     return "Plane";
    default:                        return "Unknown";
    }
}

static const char *prop_name_lookup(uint32_t prop_id)
{
    for (int i = 0; i < prop_cache_count; i++) {
        if (prop_cache[i].prop_id == prop_id)
            return prop_cache[i].name;
    }
    return NULL;
}

static const char *obj_type_lookup(uint32_t obj_id)
{
    for (int i = 0; i < obj_cache_count; i++) {
        if (obj_cache[i].obj_id == obj_id)
            return obj_type_str(obj_cache[i].obj_type);
    }
    return NULL;
}

static const uint8_t *blob_data_lookup(uint32_t blob_id, uint32_t *out_len)
{
    for (int i = 0; i < blob_cache_count; i++) {
        if (blob_cache[i].blob_id == blob_id) {
            *out_len = blob_cache[i].length;
            return blob_cache[i].data;
        }
    }
    return NULL;
}

static void cache_prop(uint32_t prop_id, const char *name, uint32_t flags)
{
    /* Update existing */
    for (int i = 0; i < prop_cache_count; i++) {
        if (prop_cache[i].prop_id == prop_id) {
            strncpy(prop_cache[i].name, name, DRM_PROP_NAME_LEN - 1);
            prop_cache[i].flags = flags;
            return;
        }
    }
    if (prop_cache_count < MAX_PROPS) {
        prop_cache[prop_cache_count].prop_id = prop_id;
        strncpy(prop_cache[prop_cache_count].name, name, DRM_PROP_NAME_LEN - 1);
        prop_cache[prop_cache_count].name[DRM_PROP_NAME_LEN - 1] = '\0';
        prop_cache[prop_cache_count].flags = flags;
        prop_cache_count++;
    }
}

static void cache_blob(uint32_t blob_id, const void *data, uint32_t length)
{
    /* Update existing */
    for (int i = 0; i < blob_cache_count; i++) {
        if (blob_cache[i].blob_id == blob_id) {
            blob_cache[i].length = length;
            uint32_t copy_len = length < 256 ? length : 256;
            memcpy(blob_cache[i].data, data, copy_len);
            return;
        }
    }
    if (blob_cache_count < MAX_BLOBS) {
        blob_cache[blob_cache_count].blob_id = blob_id;
        blob_cache[blob_cache_count].length = length;
        uint32_t copy_len = length < 256 ? length : 256;
        memcpy(blob_cache[blob_cache_count].data, data, copy_len);
        blob_cache_count++;
    }
}

static void cache_obj(uint32_t obj_id, uint32_t obj_type)
{
    for (int i = 0; i < obj_cache_count; i++) {
        if (obj_cache[i].obj_id == obj_id) {
            obj_cache[i].obj_type = obj_type;
            return;
        }
    }
    if (obj_cache_count < MAX_OBJECTS) {
        obj_cache[obj_cache_count].obj_id = obj_id;
        obj_cache[obj_cache_count].obj_type = obj_type;
        obj_cache_count++;
    }
}

/* ── Flag decoding ──────────────────────────────────────────────────── */

static void decode_atomic_flags(uint32_t flags, char *buf, size_t buflen)
{
    buf[0] = '\0';
    struct { uint32_t flag; const char *name; } bits[] = {
        { DRM_MODE_PAGE_FLIP_EVENT,    "PAGE_FLIP_EVENT" },
        { DRM_MODE_PAGE_FLIP_ASYNC,    "PAGE_FLIP_ASYNC" },
        { DRM_MODE_ATOMIC_TEST_ONLY,   "TEST_ONLY" },
        { DRM_MODE_ATOMIC_NONBLOCK,    "NONBLOCK" },
        { DRM_MODE_ATOMIC_ALLOW_MODESET, "ALLOW_MODESET" },
    };
    int first = 1;
    for (size_t i = 0; i < sizeof(bits) / sizeof(bits[0]); i++) {
        if (flags & bits[i].flag) {
            if (!first) strncat(buf, " | ", buflen - strlen(buf) - 1);
            strncat(buf, bits[i].name, buflen - strlen(buf) - 1);
            first = 0;
        }
    }
    /* Any remaining unknown bits */
    uint32_t known = DRM_MODE_PAGE_FLIP_EVENT | DRM_MODE_PAGE_FLIP_ASYNC |
                     DRM_MODE_ATOMIC_TEST_ONLY | DRM_MODE_ATOMIC_NONBLOCK |
                     DRM_MODE_ATOMIC_ALLOW_MODESET;
    uint32_t unknown = flags & ~known;
    if (unknown) {
        char tmp[32];
        snprintf(tmp, sizeof(tmp), "0x%x", unknown);
        if (!first) strncat(buf, " | ", buflen - strlen(buf) - 1);
        strncat(buf, tmp, buflen - strlen(buf) - 1);
    }
    if (first)
        strncpy(buf, "(none)", buflen - 1);
}

/* ── Mode blob decoding ─────────────────────────────────────────────── */

static void decode_mode_blob(const uint8_t *data, uint32_t len, char *buf, size_t buflen)
{
    if (len < sizeof(struct drm_mode_modeinfo)) {
        snprintf(buf, buflen, "[blob %u bytes]", len);
        return;
    }
    const struct drm_mode_modeinfo *mode = (const struct drm_mode_modeinfo *)data;
    double vrefresh = 0;
    if (mode->htotal && mode->vtotal)
        vrefresh = (double)mode->clock * 1000.0 /
                   ((double)mode->htotal * (double)mode->vtotal);
    snprintf(buf, buflen, "[blob: %ux%u@%.2f clock=%u flags=0x%x type=0x%x \"%.*s\"]",
             mode->hdisplay, mode->vdisplay, vrefresh, mode->clock,
             mode->flags, mode->type,
             DRM_DISPLAY_MODE_LEN, mode->name);
}

/* ── Sort helper for properties ─────────────────────────────────────── */

struct prop_entry {
    uint32_t prop_id;
    uint64_t value;
    const char *name; /* may be NULL */
};

static int prop_entry_cmp(const void *a, const void *b)
{
    const struct prop_entry *pa = a, *pb = b;
    /* Sort by name if both have names, otherwise by ID */
    if (pa->name && pb->name)
        return strcmp(pa->name, pb->name);
    if (pa->name) return -1;
    if (pb->name) return 1;
    return (int)pa->prop_id - (int)pb->prop_id;
}

/* ── Sort helper for objects ────────────────────────────────────────── */

struct obj_entry {
    uint32_t obj_id;
    uint32_t prop_offset; /* index into props/values arrays */
    uint32_t prop_count;
};

static int obj_entry_cmp(const void *a, const void *b)
{
    const struct obj_entry *oa = a, *ob = b;
    return (int)oa->obj_id - (int)ob->obj_id;
}

/* ── Atomic commit logger ───────────────────────────────────────────── */

static void log_atomic(const struct drm_mode_atomic *atomic, int ret, int err)
{
    if (!log_fp) return;
    if (atomic->count_objs > 10000) return; /* sanity */

    struct timeval tv;
    gettimeofday(&tv, NULL);

    int seq = ++atomic_seq;

    char flagbuf[256];
    decode_atomic_flags(atomic->flags, flagbuf, sizeof(flagbuf));

    fprintf(log_fp, "=== ATOMIC #%d @ %ld.%06ld ===\n",
            seq, (long)tv.tv_sec, (long)tv.tv_usec);
    fprintf(log_fp, "flags: %s (0x%x)\n", flagbuf, atomic->flags);

    const uint32_t *objs       = (const uint32_t *)(uintptr_t)atomic->objs_ptr;
    const uint32_t *count_props = (const uint32_t *)(uintptr_t)atomic->count_props_ptr;
    const uint32_t *props      = (const uint32_t *)(uintptr_t)atomic->props_ptr;
    const uint64_t *values     = (const uint64_t *)(uintptr_t)atomic->prop_values_ptr;

    if (!objs || !count_props || !props || !values) {
        fprintf(log_fp, "(null pointers in atomic struct)\n");
        goto done;
    }

    /* Calculate total props for bounds checking */
    uint32_t total_props = 0;
    for (uint32_t i = 0; i < atomic->count_objs; i++) {
        total_props += count_props[i];
        if (total_props > 100000) {
            fprintf(log_fp, "(too many properties, aborting log)\n");
            goto done;
        }
    }

    /* Build sorted object list */
    struct obj_entry *sorted_objs = alloca(atomic->count_objs * sizeof(struct obj_entry));
    uint32_t offset = 0;
    for (uint32_t i = 0; i < atomic->count_objs; i++) {
        sorted_objs[i].obj_id = objs[i];
        sorted_objs[i].prop_offset = offset;
        sorted_objs[i].prop_count = count_props[i];
        offset += count_props[i];
    }
    qsort(sorted_objs, atomic->count_objs, sizeof(struct obj_entry), obj_entry_cmp);

    for (uint32_t i = 0; i < atomic->count_objs; i++) {
        uint32_t oid = sorted_objs[i].obj_id;
        uint32_t poff = sorted_objs[i].prop_offset;
        uint32_t pcnt = sorted_objs[i].prop_count;

        const char *type = obj_type_lookup(oid);
        if (type)
            fprintf(log_fp, "--- Object %u (%s) ---\n", oid, type);
        else
            fprintf(log_fp, "--- Object %u ---\n", oid);

        /* Build sorted property list */
        struct prop_entry *sorted_props = alloca(pcnt * sizeof(struct prop_entry));
        for (uint32_t j = 0; j < pcnt; j++) {
            sorted_props[j].prop_id = props[poff + j];
            sorted_props[j].value = values[poff + j];
            sorted_props[j].name = prop_name_lookup(props[poff + j]);
        }
        qsort(sorted_props, pcnt, sizeof(struct prop_entry), prop_entry_cmp);

        for (uint32_t j = 0; j < pcnt; j++) {
            const char *pname = sorted_props[j].name;
            uint32_t pid = sorted_props[j].prop_id;
            uint64_t val = sorted_props[j].value;

            if (pname)
                fprintf(log_fp, "  %s [%u]", pname, pid);
            else
                fprintf(log_fp, "  prop_%u", pid);

            fprintf(log_fp, " = %lu", (unsigned long)val);

            /* Decode MODE_ID blobs inline */
            if (pname && strcmp(pname, "MODE_ID") == 0 && val != 0) {
                uint32_t blen = 0;
                const uint8_t *bdata = blob_data_lookup((uint32_t)val, &blen);
                if (bdata) {
                    char modebuf[256];
                    decode_mode_blob(bdata, blen, modebuf, sizeof(modebuf));
                    fprintf(log_fp, " %s", modebuf);
                }
            }

            /* Decode SRC_* 16.16 fixed point */
            if (pname && (strncmp(pname, "SRC_", 4) == 0) && val > 0xFFFF) {
                fprintf(log_fp, " [16.16: %lu]", (unsigned long)(val >> 16));
            }

            fprintf(log_fp, "\n");
        }
    }

done:
    if (ret == 0)
        fprintf(log_fp, "result: 0 (success)\n");
    else
        fprintf(log_fp, "result: %d (errno=%d %s)\n", ret, err, strerror(err));
    fprintf(log_fp, "===\n\n");
    fflush(log_fp);
}

/* ── Constructor ────────────────────────────────────────────────────── */

__attribute__((constructor))
static void shim_init(void)
{
    ensure_real_ioctl();

    const char *dir = getenv("DRM_SHIM_LOG_DIR");
    if (!dir) dir = "/tmp/drm-atomic-log";
    mkdir(dir, 0755);

    char path[4096];
    snprintf(path, sizeof(path), "%s/drm-atomic-%d.log", dir, getpid());
    log_fp = fopen(path, "w");
    if (log_fp) {
        setvbuf(log_fp, NULL, _IOLBF, 0);
        fprintf(log_fp, "# drm-atomic-log pid=%d\n\n", getpid());
    }
}

/* ── ioctl hook ─────────────────────────────────────────────────────── */

int ioctl(int fd, unsigned long request, ...)
{
    va_list ap;
    va_start(ap, request);
    void *arg = va_arg(ap, void *);
    va_end(ap);

    ensure_real_ioctl();

    /* Fast path: not a DRM ioctl */
    if (_IOC_TYPE(request) != DRM_IOCTL_BASE)
        return real_ioctl(fd, request, arg);

    int nr = _IOC_NR(request);

    /* DRM_IOCTL_MODE_GETPROPERTY – cache property names */
    if (nr == _IOC_NR(DRM_IOCTL_MODE_GETPROPERTY)) {
        int ret = real_ioctl(fd, request, arg);
        if (ret == 0 && arg) {
            struct drm_mode_get_property *gp = arg;
            pthread_mutex_lock(&shim_mutex);
            cache_prop(gp->prop_id, gp->name, gp->flags);
            pthread_mutex_unlock(&shim_mutex);
        }
        return ret;
    }

    /* DRM_IOCTL_MODE_CREATEPROPBLOB – cache blob data */
    if (nr == _IOC_NR(DRM_IOCTL_MODE_CREATEPROPBLOB)) {
        struct drm_mode_create_blob *cb = arg;
        /* Save data before ioctl (pointer valid now) */
        uint8_t saved_data[256];
        uint32_t saved_len = 0;
        if (cb && cb->data && cb->length > 0) {
            saved_len = cb->length < 256 ? cb->length : 256;
            memcpy(saved_data, (const void *)(uintptr_t)cb->data, saved_len);
        }
        int ret = real_ioctl(fd, request, arg);
        if (ret == 0 && saved_len > 0) {
            pthread_mutex_lock(&shim_mutex);
            cache_blob(cb->blob_id, saved_data, cb->length);
            pthread_mutex_unlock(&shim_mutex);
        }
        return ret;
    }

    /* DRM_IOCTL_MODE_OBJ_GETPROPERTIES – cache object types */
    if (nr == _IOC_NR(DRM_IOCTL_MODE_OBJ_GETPROPERTIES)) {
        int ret = real_ioctl(fd, request, arg);
        if (ret == 0 && arg) {
            struct drm_mode_obj_get_properties *op = arg;
            pthread_mutex_lock(&shim_mutex);
            cache_obj(op->obj_id, op->obj_type);
            pthread_mutex_unlock(&shim_mutex);
        }
        return ret;
    }

    /* DRM_IOCTL_MODE_ATOMIC – the main event */
    if (nr == _IOC_NR(DRM_IOCTL_MODE_ATOMIC)) {
        int ret = real_ioctl(fd, request, arg);
        int saved_errno = errno;
        if (arg) {
            pthread_mutex_lock(&shim_mutex);
            log_atomic(arg, ret, saved_errno);
            pthread_mutex_unlock(&shim_mutex);
        }
        errno = saved_errno;
        return ret;
    }

    /* Everything else: pass through */
    return real_ioctl(fd, request, arg);
}
