use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serenity::{
    async_trait,
    model::{
        guild::Member,
        gateway::Ready, prelude::{Activity, GuildId, RoleId},
    },
    prelude::*,
};
use serde_derive::Deserialize;

struct Handler {
    newbie_mapping: HashMap<u64, u64>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        ctx.set_activity(Activity::playing("with bunnies")).await;
    }

    async fn cache_ready(&self, ctx: Context, ready: Vec<GuildId>) {
        if let Err(e) = self.cache_ready_impl(ctx, ready).await {
            eprintln!("Error in cache_ready: {}", e);
        }
    }

    // This is called when a member is updated.
    async fn guild_member_update(&self, ctx: Context, _old: Option<Member>, new: Member) {
        if let Err(e) = self.update_roles(&ctx, new).await {
            eprintln!("Error in guild_member_update: {}", e);
        }
    }
}

impl Handler {
    async fn cache_ready_impl(&self, ctx: Context, ready: Vec<GuildId>) -> Result<()> {
        for guild in ready {
            if let Some(guild) = guild.to_guild_cached(&ctx) {
                println!("Guild: {}", guild.name);
                println!("- Members: {}", guild.member_count);
                println!("- Roles: {}", guild.roles.len());
                
                // Run the role update for all members in the guild.
                let members = guild.members(&ctx, None, None).await?;
                for member in members {
                    self.update_roles(&ctx, member).await?;
                }
            }
        }
        println!("First pass complete. Now listening for events...");

        Ok(())
    }

    async fn update_roles(&self, ctx: &Context, mut member: Member) -> Result<()> {
        let newbie_role = self.newbie_mapping.get(&member.guild_id.0)
            .map(|id| RoleId(*id))
            .ok_or(anyhow!("No newbie role mapping for guild {}", member.guild_id.0))?;

        let roles = member.roles.clone();
        if roles.len() == 0 {
            println!("Adding newbie role to {}", member.user.name);
            member.add_role(&ctx.http, newbie_role).await?;
        } else if roles.len() > 1 && roles.contains(&newbie_role) {
            println!("Removing newbie role from {}", member.user.name);
            member.remove_role(&ctx.http, newbie_role).await?;
        }
    
        Ok(())
    }
}


#[derive(Deserialize)]
struct Config {
    discord_token: String,
    newbie_mapping: HashMap<u64, u64>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // Load config from command line argument or default to config.json.
    let config_path = std::env::args().nth(1).unwrap_or_else(|| "config.json".to_string());
    let config: Config = {
        let config_file = std::fs::File::open(&config_path)?;
        let config_reader = std::io::BufReader::new(config_file);
        serde_json::from_reader(config_reader)?
    };

    let intents = GatewayIntents::non_privileged()
        | GatewayIntents::GUILD_MEMBERS;

    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(Handler {
            newbie_mapping: config.newbie_mapping,
        })
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        eprintln!("Client error: {:?}", why);
    }

    Ok(())
}
