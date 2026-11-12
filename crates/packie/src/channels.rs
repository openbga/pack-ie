//! `packie channels`: pause, resume and restart a channel (`docs/spec/operator-console.md` §5).

use clap::{Args, Subcommand};
use packie_core::ChannelId;
use packie_diagnostics::{ChannelSnapshot, EngineSnapshot};
use packie_ipc::{ChannelCommand, Request, Response};

use crate::Failure;
use crate::client::{Target, ask, ask_done, unexpected};
use crate::output::print_line;

#[derive(Debug, Subcommand)]
pub(crate) enum ChannelsCommand {
    /// Stop a channel until it is resumed: a listener refuses its sender, accepted messages
    /// stay queued, and no retry is scheduled.
    Pause(ChannelArgs),
    /// Start a paused channel from the active configuration.
    Resume(ChannelArgs),
    /// Stop a channel and start it again from the active definition; its session closes and
    /// its counters reset.
    Restart(ChannelArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ChannelArgs {
    /// The channel id, as `packie status` prints it.
    channel: ChannelId,
}

pub(crate) async fn run(target: &Target, command: ChannelsCommand) -> Result<(), Failure> {
    let (command, args) = match command {
        ChannelsCommand::Pause(args) => (ChannelCommand::Pause, args),
        ChannelsCommand::Resume(args) => (ChannelCommand::Resume, args),
        ChannelsCommand::Restart(args) => (ChannelCommand::Restart, args),
    };
    let client = target.connect().await?;
    let request = Request::ChannelCommand {
        channel_id: args.channel,
        command,
    };
    // The engine rejects a command the channel's state does not allow, with the reason.
    ask_done(&client, request).await?;
    let Response::Snapshot { snapshot } = ask(&client, Request::GetSnapshot).await? else {
        return Err(unexpected());
    };
    print_line(&outcome(command, args.channel, &snapshot));
    Ok(())
}

/// What the command did and what to check next, from the snapshot taken right after.
fn outcome(command: ChannelCommand, id: ChannelId, snapshot: &EngineSnapshot) -> String {
    let channel = snapshot.channels.iter().find(|c| c.id == id);
    match command {
        ChannelCommand::Pause => format!(
            "Channel {id} paused. No retry is scheduled; messages already accepted stay queued. \
             `packie channels resume {id}` starts it again."
        ),
        ChannelCommand::Resume => format!(
            "Channel {id} resumed, starting from configuration version {}.{}",
            channel.map_or_else(|| "?".to_owned(), |c| c.config_version.to_string()),
            state_note(channel)
        ),
        ChannelCommand::Restart => format!(
            "Channel {id} restarted from the active definition; its counters were reset.{}",
            state_note(channel)
        ),
    }
}

/// ` It is now starting.` plus where to look.
fn state_note(channel: Option<&ChannelSnapshot>) -> String {
    channel.map_or_else(String::new, |c| {
        format!(" It is now {}; `packie status` follows it.", c.state)
    })
}
