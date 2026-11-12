//! `packie secrets`: store, remove and list the secrets endpoints reference as
//! `{"secret": "<name>"}` (`docs/spec/configuration.md` §11). Values go to the engine's secret
//! provider and are never printed.

use std::io::{IsTerminal, Read};

use clap::Subcommand;
use packie_ipc::{Request, Response};

use crate::Failure;
use crate::client::{Target, ask, ask_done, unexpected};
use crate::output::{print_json, print_line};

#[derive(Debug, Subcommand)]
pub(crate) enum SecretsCommand {
    /// List the names of stored secrets (never their values).
    List {
        /// Print the names as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Store a secret. The value is asked for without echo, or read from standard input when
    /// it is piped (`packie secrets set sftp-password < file`); it is never an argument.
    Set {
        /// The name endpoints reference, `^[a-z0-9][a-z0-9_.-]{0,62}$`.
        name: String,
    },
    /// Remove a secret.
    Delete {
        /// The secret's name.
        name: String,
    },
}

pub(crate) async fn run(target: &Target, command: SecretsCommand) -> Result<(), Failure> {
    match command {
        SecretsCommand::List { json } => list(target, json).await,
        SecretsCommand::Set { name } => {
            let value = read_value(&name)?;
            let client = target.connect().await?;
            let request = Request::SetSecret {
                name: name.clone(),
                value,
            };
            ask_done(&client, request).await?;
            print_line(&format!(
                "Secret {name} stored. Endpoints that reference it use the new value from their \
                 next connection."
            ));
            Ok(())
        }
        SecretsCommand::Delete { name } => {
            let client = target.connect().await?;
            let request = Request::DeleteSecret { name: name.clone() };
            ask_done(&client, request).await?;
            print_line(&format!(
                "Secret {name} removed. Endpoints that still reference it cannot connect until \
                 it is set again."
            ));
            Ok(())
        }
    }
}

async fn list(target: &Target, json: bool) -> Result<(), Failure> {
    let client = target.connect().await?;
    let Response::Secrets { names } = ask(&client, Request::ListSecrets).await? else {
        return Err(unexpected());
    };
    if json {
        return print_json(&serde_json::json!({ "names": names }));
    }
    if names.is_empty() {
        print_line("No secrets stored. `packie secrets set <name>` adds one.");
    }
    for name in names {
        print_line(&name);
    }
    Ok(())
}

/// The value: asked for without echo on a terminal, else all of standard input without its
/// final line break.
fn read_value(name: &str) -> Result<String, Failure> {
    let value = if std::io::stdin().is_terminal() {
        rpassword::prompt_password(format!("Value for {name} (not shown): "))
            .map_err(|e| Failure::new(format!("cannot read the value: {e}")))?
    } else {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| Failure::new(format!("cannot read the value from standard input: {e}")))?;
        strip_line_break(text)
    };
    if value.is_empty() {
        return Err(Failure::new("The value is empty; nothing was stored."));
    }
    Ok(value)
}

fn strip_line_break(mut text: String) -> String {
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_final_line_break_is_removed() {
        assert_eq!(strip_line_break("s3cr3t\n".into()), "s3cr3t");
        assert_eq!(strip_line_break("s3cr3t\r\n".into()), "s3cr3t");
        assert_eq!(strip_line_break("two\nlines\n\n".into()), "two\nlines\n");
        assert_eq!(strip_line_break(" spaced ".into()), " spaced ");
    }
}
