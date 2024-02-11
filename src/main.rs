#![warn(clippy::str_to_string)]

use crate::commands::*;
use anyhow::anyhow;
use poise::serenity_prelude as serenity;
use shuttle_secrets::SecretStore;
use shuttle_serenity::ShuttleSerenity;

pub mod commands;
pub mod responses;
pub mod utils;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

pub struct Data {
    db: mysql::Pool,
    dnd_role: serenity::RoleId,
}

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) -> () {
    match error {
        poise::FrameworkError::Setup { error, .. } => {
            panic!("Failed to build framework: {:?}", error)
        }
        poise::FrameworkError::Command { error, ctx, .. } => {
            println!(
                "Error in command `{}`: {:?}",
                ctx.command().qualified_name,
                error
            );

            responses::failure(ctx, "Something went wrong.")
                .await
                .unwrap_or_default();
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                println!("Failed to call on_error: {:?}", e);
            }
        }
    }
}

async fn on_event(
    _ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    _data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Ready { data_about_bot } => {
            println!("{} is connected!", data_about_bot.user.name);
        }
        _ => {}
    }

    Ok(())
}

#[shuttle_runtime::main]
pub async fn poise(#[shuttle_secrets::Secrets] secret_store: SecretStore) -> ShuttleSerenity {
    let token = if let Some(token) = secret_store.get("DISCORD_TOKEN") {
        token
    } else {
        return Err(anyhow!("DISCORD_TOKEN not found in secret store").into());
    };

    let database_url = if let Some(url) = secret_store.get("DATABASE_URL") {
        url
    } else {
        return Err(anyhow!("DATABASE_URL not found in secret store").into());
    };

    let commands = vec![
        help::help(),
        settings::settings(),
        dnd::campaign::session::session(),
        dnd::dice::roll(),
    ];

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            event_handler: |_ctx, event, _framework, _data| {
                Box::pin(on_event(_ctx, event, _framework, _data))
            },
            commands,
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".into()),
                edit_tracker: Some(Into::into(poise::EditTracker::for_timespan(
                    std::time::Duration::from_secs(60),
                ))),
                ..Default::default()
            },
            on_error: |error| Box::pin(on_error(error)),
            command_check: Some(|ctx| {
                Box::pin(async move {
                    Ok(ctx
                        .author_member()
                        .await
                        .unwrap()
                        .roles
                        .contains(&serenity::RoleId::from(ctx.data().dnd_role)))
                })
            }),
            ..Default::default()
        })
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    db: utils::db::init_dnd_db(&database_url),
                    dnd_role: serenity::RoleId::from(901464574530814002), // TODO: Change this from
                                                                          // a harcoded role from
                                                                          // the str::from_utf8
                                                                          // server
                })
            })
        })
        .build();

    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS;

    let client = serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await
        .map_err(shuttle_runtime::CustomError::new)?;

    Ok(client.into())
}
