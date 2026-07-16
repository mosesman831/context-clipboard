//! Request dispatch. Each IPC `Request` is mapped to a `Response`, touching the
//! real DB where possible. Errors are converted into `Response::Err` rather
//! than propagated so a single bad request never tears down a connection.

use crate::db;
use crate::state::AppState;
use clipboard_core::ipc::{ExcludedAppInfo, Request, Response, IPC_VERSION};
use clipboard_core::schema as cs;
use std::sync::Arc;
use tracing::warn;

/// Capabilities advertised for this build/platform (SPEC §6 matrix). Reported
/// as a string list in `Response::Status`.
fn platform_caps() -> Vec<String> {
    // Text capture works on X11/macOS here; window title, image, hotkey, and
    // keychain enrichers are future work (see report TODOs).
    let mut caps = vec!["clipboard_text".to_string()];
    caps.push(format!("os:{}", std::env::consts::OS));
    caps
}

pub fn dispatch(state: &Arc<AppState>, req: Request) -> Response {
    match handle(state, req) {
        Ok(resp) => resp,
        Err(e) => {
            // Errors may include DB messages but never clipboard bodies.
            warn!(error = %e, "request failed");
            Response::error("internal", e.to_string())
        }
    }
}

fn handle(state: &Arc<AppState>, req: Request) -> anyhow::Result<Response> {
    match req {
        Request::Ping { v } => {
            if v != IPC_VERSION {
                warn!(
                    client_version = v,
                    server_version = IPC_VERSION,
                    "IPC version mismatch"
                );
            }
            Ok(Response::Ok)
        }

        Request::GetStatus => {
            let count = {
                let conn = lock_db(state)?;
                cs::count_clips(&conn)?
            };
            Ok(Response::Status {
                paused: state.is_paused(),
                count: count as u64,
                version: IPC_VERSION,
                platform_caps: platform_caps(),
            })
        }

        Request::Recent { limit } => {
            let limit = clamp_limit(limit.unwrap_or_else(|| state.recent_count()));
            let conn = lock_db(state)?;
            let items = db::recent(&conn, limit)?;
            Ok(Response::SearchResults { items })
        }

        Request::Search {
            query,
            limit,
            category,
            app,
            since,
        } => {
            let limit = clamp_limit(limit.unwrap_or(50));
            let conn = lock_db(state)?;
            let items = db::search(
                &conn,
                &query,
                limit,
                category.as_deref(),
                app.as_deref(),
                since.as_deref(),
            )?;
            Ok(Response::SearchResults { items })
        }

        Request::Get { id } => {
            let conn = lock_db(state)?;
            match db::get_detail(&conn, &state.key, &id)? {
                Some(detail) => Ok(Response::ClipDetail {
                    detail: Box::new(detail),
                }),
                None => Ok(Response::error(
                    "not_found",
                    format!("no clip with id {id}"),
                )),
            }
        }

        Request::Delete { id } => {
            let conn = lock_db(state)?;
            if db::delete(&conn, &state.key, &id)? {
                Ok(Response::Ok)
            } else {
                Ok(Response::error(
                    "not_found",
                    format!("no clip with id {id}"),
                ))
            }
        }

        Request::Clear => {
            let conn = lock_db(state)?;
            cs::clear_clips(&conn)?;
            Ok(Response::Ok)
        }

        Request::SetPaused { paused } => {
            state.set_paused(paused);
            let conn = lock_db(state)?;
            let _ = cs::set_setting(&conn, "capture.paused", &paused.to_string());
            Ok(Response::Ok)
        }

        Request::UpdateSettings { settings } => {
            let conn = lock_db(state)?;
            apply_settings(&conn, &settings)?;
            Ok(Response::Ok)
        }

        Request::ListExcludedApps => {
            let conn = lock_db(state)?;
            let apps = cs::list_excluded_apps(&conn)?
                .into_iter()
                .map(|a| ExcludedAppInfo {
                    id: a.id,
                    match_type: a.match_type,
                    match_value: a.match_value,
                })
                .collect();
            Ok(Response::ExcludedApps { apps })
        }

        Request::AddExcludedApp {
            match_type,
            match_value,
        } => {
            let conn = lock_db(state)?;
            match cs::add_excluded_app(&conn, &match_type, &match_value) {
                Ok(_) => Ok(Response::Ok),
                Err(e) => Ok(Response::error("invalid_input", e.to_string())),
            }
        }

        Request::RemoveExcludedApp { id } => {
            let conn = lock_db(state)?;
            if cs::remove_excluded_app(&conn, id)? {
                Ok(Response::Ok)
            } else {
                Ok(Response::error(
                    "not_found",
                    format!("no excluded app with id {id}"),
                ))
            }
        }
    }
}

/// Persist a JSON object of settings as `settings` table rows. Non-string
/// values are stored via their JSON representation.
fn apply_settings(conn: &rusqlite::Connection, settings: &serde_json::Value) -> anyhow::Result<()> {
    let Some(obj) = settings.as_object() else {
        anyhow::bail!("settings must be a JSON object");
    };
    for (k, v) in obj {
        let value = match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        cs::set_setting(conn, k, &value)?;
    }
    Ok(())
}

fn lock_db(
    state: &Arc<AppState>,
) -> anyhow::Result<std::sync::MutexGuard<'_, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|_| anyhow::anyhow!("database mutex poisoned"))
}

fn clamp_limit(limit: u32) -> u32 {
    limit.clamp(1, 1000)
}
