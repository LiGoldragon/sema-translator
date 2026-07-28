use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use sema_translator::{Runtime, StaticAuthorizationPolicy, principal_for_unix_uid};

struct Configuration {
    socket: PathBuf,
    database: PathBuf,
    authorized_uid: u32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let configuration = parse_configuration(std::env::args().skip(1))?;
    let authorization = StaticAuthorizationPolicy::new()
        .grant_all(principal_for_unix_uid(configuration.authorized_uid));
    let listener = sema_translator::wire::bind(&configuration.socket)?;
    // Socket ownership is acquired before the database opens, so a second
    // process cannot touch the embedded store before discovering the live
    // authority. The listener's drop guard removes this exact socket inode if
    // database validation prevents readiness.
    let runtime = Runtime::open(&configuration.database, Arc::new(authorization)).await?;
    println!("READY {}", configuration.socket.display());
    std::io::stdout().flush()?;
    sema_translator::wire::serve_listener(listener, runtime).await?;
    Ok(())
}

fn parse_configuration(
    mut arguments: impl Iterator<Item = String>,
) -> Result<Configuration, Box<dyn std::error::Error>> {
    if arguments.next().as_deref() != Some("daemon") {
        return Err(usage().into());
    }
    let mut socket = None;
    let mut database = None;
    let mut authorized_uid = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--socket" => socket = Some(PathBuf::from(required_value(&mut arguments, "--socket")?)),
            "--database" => {
                database = Some(PathBuf::from(required_value(&mut arguments, "--database")?))
            }
            "--authorized-uid" => {
                authorized_uid = Some(
                    required_value(&mut arguments, "--authorized-uid")?
                        .parse::<u32>()
                        .map_err(|_| "authorized UID must be an unsigned 32-bit integer")?,
                )
            }
            _ => return Err(format!("unknown argument {argument}\n{}", usage()).into()),
        }
    }
    Ok(Configuration {
        socket: socket.ok_or_else(usage)?,
        database: database.ok_or_else(usage)?,
        authorized_uid: authorized_uid.ok_or_else(usage)?,
    })
}

fn required_value(
    arguments: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    arguments
        .next()
        .ok_or_else(|| format!("missing value for {flag}").into())
}

fn usage() -> String {
    "usage: sema-translator-daemon daemon --socket PATH --database PATH --authorized-uid UID".into()
}
