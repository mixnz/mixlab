//! `mix status` and `mix storage`.

use std::path::Path;
use std::process::ExitCode;

use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{DaemonStatus, Error, ErrorCode, StorageReport, rpc};

use crate::autostart::Autostart;
use crate::client::{Client, ask};
use crate::output::emit;
use crate::{autostart, render};

/// `mix storage`: where this home's growing directories are — roadmap task **T145**.
///
/// **Neither asynchronous nor connected**, which is what makes it the odd one in the match above.
/// Every other command sends a request; this one runs `mixengined --storage` and reads the document
/// it printed. It has to: whether the choice is still free is a count of rows, `mix` links neither
/// `mixengine-core` nor `sqlx`, and there may be no daemon — a person asking this is usually asking
/// it *before* the first start.
///
/// `--json` hands back what the daemon printed, unchanged. Re-serialising a document this client
/// only parsed in order to print it would be a second chance to render it differently.
pub(crate) fn storage(root: &Path, json: bool) -> Result<ExitCode, Error> {
    let answered = autostart::ask(root, &["--storage"])?;

    if json {
        println!("{answered}");
        return Ok(ExitCode::SUCCESS);
    }

    let report: StorageReport = serde_json::from_str(&answered).map_err(|source| {
        Error::new(
            ErrorCode::Internal,
            format!("mixengined described this home in a way this `mix` cannot read: {source}"),
        )
        .with_hint(
            "the two binaries are from different releases; reinstall MixEngine so that \
                    `mix` and `mixengined` come from one",
        )
    })?;

    print!("{}", render::storage(&report));

    Ok(ExitCode::SUCCESS)
}

/// `mix status`: what the daemon says about itself.
pub(crate) async fn status(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;
    let status: DaemonStatus = ask(&mut client, rpc::method::DAEMON_STATUS, None).await?;

    match json {
        // The newline is here rather than in `render`, which builds a document and does not know
        // whether it is the last thing on the stream. The human rendering ends in one already.
        true => emit(&format!("{}\n", render::status_json(&status)))?,
        false => emit(&render::status(&status))?,
    }

    Ok(ExitCode::SUCCESS)
}
