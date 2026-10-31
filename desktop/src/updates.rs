//! Updates from the landing page's feeds, one per channel: stable has
//! production releases only, beta the newest beta or production release.
//! Dev builds never update themselves.
//!
//! A check keeps the update it finds, and installing installs that one, so
//! you install what the dialog showed. Before Tandem quits for the update,
//! changed projects are backed up, as on any quit.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use tandem_board_core::db::UpdateChannel;
use tauri::AppHandle;
use tauri::Url;
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::build_env::BuildEnv;
use crate::error::{Error, describe};
use crate::links::SITE;
use crate::projects::Projects;

/// An update you can install, as the update dialog shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// Such as `0.2.0`.
    pub version: String,
    /// When it was released, in RFC 3339 form.
    pub date: Option<String>,
    /// What changed, in Markdown.
    pub notes: Option<String>,
    /// False when this install can't replace itself, as a `.deb` or `.rpm`
    /// install can't; the dialog then links to the downloads.
    pub can_install: bool,
}

/// The update found by the last check, if any.
#[derive(Default)]
pub struct Updates {
    found: Mutex<Option<Update>>,
}

/// The channel a build updates from: the one Settings picked, or the
/// build's own. `None` for builds that never update.
pub(crate) fn channel(env: BuildEnv, picked: Option<UpdateChannel>) -> Option<UpdateChannel> {
    env.update_channel().map(|own| picked.unwrap_or(own))
}

/// The feed of a channel on the landing page.
pub(crate) fn feed_url(channel: UpdateChannel) -> String {
    format!("{SITE}/updates/{}.json", channel.as_str())
}

/// Whether the updater can replace this install. On Linux it can only
/// replace an AppImage, which runs with `APPIMAGE` set.
fn can_install() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some()
}

fn update_error(error: &tauri_plugin_updater::Error) -> Error {
    Error::new("update_failed", describe(error))
}

impl Updates {
    /// Asks `channel`'s feed for a version newer than this one, and keeps
    /// it for [`install`](Self::install).
    pub async fn check(
        &self,
        app: &AppHandle,
        projects: Arc<Projects>,
        channel: UpdateChannel,
    ) -> Result<Option<UpdateInfo>, Error> {
        let url = Url::parse(&feed_url(channel)).map_err(|e| Error::internal(&e))?;
        let updater = app
            .updater_builder()
            .endpoints(vec![url])
            .map_err(|e| update_error(&e))?
            // The Windows installer ends the app without the usual quit, so
            // back up here instead.
            .on_before_exit(move || projects.back_up_changed(SignedDuration::ZERO))
            .build()
            .map_err(|e| update_error(&e))?;
        let update = updater.check().await.map_err(|e| update_error(&e))?;
        let info = update.as_ref().map(|update| UpdateInfo {
            version: update.version.clone(),
            date: update
                .date
                .and_then(|date| Timestamp::from_second(date.unix_timestamp()).ok())
                .map(|date| date.to_string()),
            notes: update.body.clone().filter(|notes| !notes.trim().is_empty()),
            can_install: can_install(),
        });
        *self.lock() = update;
        Ok(info)
    }

    /// Downloads and installs the update the last check found, then
    /// restarts into it. Refused with `no_update` if there is none.
    pub async fn install(&self, app: &AppHandle) -> Result<(), Error> {
        if !can_install() {
            return Err(Error::new(
                "cannot_install",
                "this install can't update itself; download the new version instead",
            ));
        }
        let update = self
            .lock()
            .clone()
            .ok_or_else(|| Error::new("no_update", "there is no update to install"))?;
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| update_error(&e))?;
        // Windows never gets here: its installer has ended the app. Elsewhere
        // the new version is in place, and the restart quits as usual, with
        // its backups, before starting it.
        app.request_restart();
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, Option<Update>> {
        self.found.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_pick_the_channel_only_for_builds_that_update() {
        assert_eq!(
            channel(BuildEnv::Production, None),
            Some(UpdateChannel::Stable)
        );
        assert_eq!(channel(BuildEnv::Beta, None), Some(UpdateChannel::Beta));
        assert_eq!(
            channel(BuildEnv::Production, Some(UpdateChannel::Beta)),
            Some(UpdateChannel::Beta)
        );
        assert_eq!(
            channel(BuildEnv::Beta, Some(UpdateChannel::Stable)),
            Some(UpdateChannel::Stable)
        );
        assert_eq!(channel(BuildEnv::Dev, Some(UpdateChannel::Stable)), None);
    }

    #[test]
    fn each_channel_has_a_feed_on_the_landing_page() {
        assert_eq!(
            feed_url(UpdateChannel::Stable),
            "https://tandem.laszlomocsy.com/updates/stable.json"
        );
        assert_eq!(
            feed_url(UpdateChannel::Beta),
            "https://tandem.laszlomocsy.com/updates/beta.json"
        );
    }
}
