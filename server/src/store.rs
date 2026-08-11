//! SQLite persistence.
//!
//! Messages *are* the log: one append-only table with a per-chat monotonic
//! `seq`, which is all a reconnecting client needs to catch up. Status events
//! never reach this module — see [`rebeam_core::Event::is_durable`].

use std::{
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;
use rebeam_core::{
    Approval, ApprovalDecision, ApprovalDisplay, ApprovalState, Chat, ChatKind, HistoryGrant,
    Invite, Member, MemberKind, Membership, Message, MessageKind, Presence, Trigger,
};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

const ACCESS_TTL_MS: i64 = 60 * 60 * 1_000;
const REFRESH_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1_000;
const DEVICE_TTL_MS: i64 = 10 * 60 * 1_000;
const DEVICE_POLL_INTERVAL_SECS: i64 = 2;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSession {
    pub token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user: User,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingCode {
    pub code: String,
}

#[derive(Debug)]
pub enum AuthError {
    DuplicateEmail,
    InvalidCredentials,
    RateLimited,
    Password,
    Database(rusqlite::Error),
}

impl From<rusqlite::Error> for AuthError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineRecord {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_seen: Option<i64>,
    pub online: bool,
    pub runtimes: Vec<serde_json::Value>,
    pub runtime_updated_at: Option<i64>,
}

#[derive(Debug)]
pub enum ApprovalCreate {
    Created(Approval),
    Existing(Approval),
    Conflict,
    TooMany,
}

#[derive(Debug)]
pub enum ApprovalTransition {
    Updated(Approval),
    Expired(Approval),
    DigestMismatch(Approval),
    NotPending,
    NotFound,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    pub expires_at: i64,
    pub interval_seconds: i64,
}

#[derive(Debug, Clone)]
pub enum DevicePoll {
    Pending {
        interval_seconds: i64,
    },
    SlowDown {
        interval_seconds: i64,
    },
    Authorized {
        token: String,
        machine: MachineRecord,
    },
    Expired,
    Invalid,
}

struct PendingDeviceAuthorization {
    machine_name: String,
    expires_at: i64,
    interval_seconds: i64,
    approved_by: Option<String>,
    consumed_at: Option<i64>,
    last_polled_at: Option<i64>,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.seed()?;
        Ok(store)
    }

    // -- authentication ---------------------------------------------------

    /// Development-only compatibility path. The HTTP handler refuses this in
    /// production unless it has been explicitly enabled.
    pub fn bootstrap_workspace(&self) -> rusqlite::Result<AuthSession> {
        let id = format!("u_{}", uuid::Uuid::new_v4().simple());
        let user = User {
            id: id.clone(),
            email: format!("{id}@local.invalid"),
            name: "Local workspace".into(),
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (id, email, name, password_hash, created_at) VALUES (?1, ?2, ?3, '', ?4)",
            params![user.id, user.email, user.name, now_millis()],
        )?;
        Self::ensure_user_member(&conn, &user)?;
        Self::new_session(&conn, user)
    }

    pub fn register(
        &self,
        email: &str,
        name: &str,
        password: &str,
    ) -> Result<AuthSession, AuthError> {
        let password_hash = hash_password(password)?;
        let user = User {
            id: format!("u_{}", uuid::Uuid::new_v4().simple()),
            email: normalize_email(email),
            name: name.trim().to_string(),
        };
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        if let Err(error) = tx.execute(
            "INSERT INTO users (id, email, name, password_hash, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![user.id, user.email, user.name, password_hash, now_millis()],
        ) {
            if is_constraint(&error) {
                return Err(AuthError::DuplicateEmail);
            }
            return Err(AuthError::Database(error));
        }
        Self::ensure_user_member(&tx, &user)?;
        let session = Self::new_session(&tx, user)?;
        tx.commit()?;
        Ok(session)
    }

    pub fn login(&self, email: &str, password: &str) -> Result<AuthSession, AuthError> {
        let email = normalize_email(email);
        let now = now_millis();
        let (user, encoded): (User, String) = {
            let conn = self.conn.lock().unwrap();
            let recent_failures: i64 = conn.query_row(
                "SELECT COUNT(*) FROM login_failures
                 WHERE email = ?1 AND failed_at >= ?2",
                params![email, now - 15 * 60 * 1_000],
                |row| row.get(0),
            )?;
            if recent_failures >= 10 {
                return Err(AuthError::RateLimited);
            }
            let found = conn
                .query_row(
                    "SELECT id, email, name, password_hash FROM users WHERE email = ?1",
                    params![email],
                    |row| {
                        Ok((
                            User {
                                id: row.get(0)?,
                                email: row.get(1)?,
                                name: row.get(2)?,
                            },
                            row.get(3)?,
                        ))
                    },
                )
                .optional()?;
            let Some(found) = found else {
                conn.execute(
                    "INSERT INTO login_failures (email, failed_at) VALUES (?1, ?2)",
                    params![email, now],
                )?;
                return Err(AuthError::InvalidCredentials);
            };
            found
        };

        if !verify_password(password, &encoded) {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO login_failures (email, failed_at) VALUES (?1, ?2)",
                params![email, now],
            )?;
            return Err(AuthError::InvalidCredentials);
        }

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM login_failures WHERE email = ?1",
            params![email],
        )?;
        Self::ensure_user_member(&conn, &user)?;
        Ok(Self::new_session(&conn, user)?)
    }

    pub fn refresh_session(&self, refresh_token: &str) -> Result<AuthSession, AuthError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let user = tx
            .query_row(
                "SELECT users.id, users.email, users.name
                 FROM sessions JOIN users ON users.id = sessions.user_id
                 WHERE sessions.refresh_token_hash = ?1
                   AND sessions.refresh_expires_at > ?2",
                params![token_hash(refresh_token), now_millis()],
                |row| {
                    Ok(User {
                        id: row.get(0)?,
                        email: row.get(1)?,
                        name: row.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or(AuthError::InvalidCredentials)?;
        tx.execute(
            "DELETE FROM sessions WHERE refresh_token_hash = ?1",
            params![token_hash(refresh_token)],
        )?;
        let session = Self::new_session(&tx, user)?;
        tx.commit()?;
        Ok(session)
    }

    pub fn user_for_token(&self, token: &str) -> rusqlite::Result<Option<User>> {
        let conn = self.conn.lock().unwrap();
        let user = conn
            .query_row(
                "SELECT users.id, users.email, users.name
             FROM sessions JOIN users ON users.id = sessions.user_id
             WHERE sessions.token_hash = ?1 AND sessions.expires_at > ?2",
                params![token_hash(token), now_millis()],
                |r| {
                    Ok(User {
                        id: r.get(0)?,
                        email: r.get(1)?,
                        name: r.get(2)?,
                    })
                },
            )
            .optional()?;
        if let Some(user) = &user {
            Self::ensure_user_member(&conn, user)?;
        }
        Ok(user)
    }

    pub fn revoke_session(&self, token: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM sessions WHERE token_hash = ?1",
            params![token_hash(token)],
        )?;
        Ok(())
    }

    pub fn machine_for_token(&self, token: &str) -> rusqlite::Result<Option<User>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT users.id, users.email, users.name FROM machine_tokens
             JOIN users ON users.id = machine_tokens.workspace_id
             WHERE machine_tokens.token_hash = ?1 AND machine_tokens.revoked_at IS NULL",
            params![token_hash(token)],
            |r| {
                Ok(User {
                    id: r.get(0)?,
                    email: r.get(1)?,
                    name: r.get(2)?,
                })
            },
        )
        .optional()
    }

    pub fn agent_belongs_to(&self, agent: &str, workspace: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT 1 FROM members WHERE id = ?1 AND kind = 'agent' AND owner_id = ?2",
                params![agent, workspace],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Resolve an agent handle within its owning workspace. Handles are
    /// globally human-readable, so seeded/legacy agents may share a name.
    pub fn find_agent_for_owner(
        &self,
        needle: &str,
        owner_id: &str,
    ) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM members
             WHERE kind = 'agent' AND owner_id = ?2
               AND (id = ?1 COLLATE NOCASE OR name = ?1 COLLATE NOCASE)
             LIMIT 1",
            params![needle, owner_id],
            |row| row.get(0),
        )
        .optional()
    }

    pub fn machine_status(&self, workspace: &str) -> rusqlite::Result<(i64, i64)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(last_seen >= ?2), 0)
                        FROM machine_tokens
                        WHERE workspace_id = ?1 AND revoked_at IS NULL",
            params![workspace, now_millis() - 45_000],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    pub fn touch_machine(&self, token: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE machine_tokens SET last_seen = ?2
             WHERE token_hash = ?1 AND revoked_at IS NULL",
            params![token_hash(token), now_millis()],
        )? > 0)
    }

    pub fn create_pairing(&self, workspace: &str) -> rusqlite::Result<PairingCode> {
        let code = format!(
            "RP-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12].to_ascii_uppercase()
        );
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO pairings (code, workspace_id, expires_at) VALUES (?1, ?2, ?3)",
            params![code, workspace, now_millis() + 600_000],
        )?;
        Ok(PairingCode { code })
    }

    pub fn redeem_pairing(&self, code: &str) -> rusqlite::Result<Result<String, String>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let found: Option<(String, i64, Option<i64>)> = tx
            .query_row(
                "SELECT workspace_id, expires_at, redeemed_at FROM pairings WHERE code = ?1",
                params![code],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((workspace, expires, redeemed)) = found else {
            return Ok(Err("no such pairing code".into()));
        };
        if redeemed.is_some() || now_millis() >= expires {
            return Ok(Err("that pairing code has expired".into()));
        }
        let token = format!("rm_{}", uuid::Uuid::new_v4().simple());
        tx.execute(
            "UPDATE pairings SET redeemed_at = ?2 WHERE code = ?1",
            params![code, now_millis()],
        )?;
        let machine_id = format!("machine_{}", uuid::Uuid::new_v4().simple());
        tx.execute(
            "INSERT INTO machine_tokens
               (token_hash, workspace_id, created_at, last_seen, id, name)
             VALUES (?1, ?2, ?3, ?3, ?4, 'Paired machine')",
            params![token_hash(&token), workspace, now_millis(), machine_id],
        )?;
        tx.commit()?;
        Ok(Ok(token))
    }

    pub fn start_device_authorization(
        &self,
        machine_name: &str,
    ) -> rusqlite::Result<DeviceAuthorization> {
        let device_code = format!("rd_{}", uuid::Uuid::new_v4().simple());
        let raw = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .to_ascii_uppercase();
        let user_code = format!("RBM-{}-{}-{}", &raw[..4], &raw[4..8], &raw[8..12]);
        let expires_at = now_millis() + DEVICE_TTL_MS;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO device_authorizations
               (device_code_hash, user_code, machine_name, expires_at, interval_seconds, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                token_hash(&device_code),
                user_code,
                machine_name.trim(),
                expires_at,
                DEVICE_POLL_INTERVAL_SECS,
                now_millis()
            ],
        )?;
        Ok(DeviceAuthorization {
            device_code,
            user_code,
            expires_at,
            interval_seconds: DEVICE_POLL_INTERVAL_SECS,
        })
    }

    pub fn approve_device(&self, user_code: &str, owner: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE device_authorizations
             SET approved_by = ?2, approved_at = ?3
             WHERE user_code = ?1 COLLATE NOCASE
               AND approved_by IS NULL
               AND consumed_at IS NULL
               AND expires_at > ?3",
            params![user_code.trim(), owner, now_millis()],
        )? > 0)
    }

    pub fn poll_device_authorization(&self, device_code: &str) -> rusqlite::Result<DevicePoll> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let found: Option<PendingDeviceAuthorization> = tx
            .query_row(
                "SELECT machine_name, expires_at, interval_seconds, approved_by,
                        consumed_at, last_polled_at
                 FROM device_authorizations WHERE device_code_hash = ?1",
                params![token_hash(device_code)],
                |row| {
                    Ok(PendingDeviceAuthorization {
                        machine_name: row.get(0)?,
                        expires_at: row.get(1)?,
                        interval_seconds: row.get(2)?,
                        approved_by: row.get(3)?,
                        consumed_at: row.get(4)?,
                        last_polled_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        let Some(found) = found else {
            return Ok(DevicePoll::Invalid);
        };
        let now = now_millis();
        if found.consumed_at.is_some() {
            return Ok(DevicePoll::Invalid);
        }
        if now >= found.expires_at {
            return Ok(DevicePoll::Expired);
        }
        if found
            .last_polled_at
            .is_some_and(|last| now - last < found.interval_seconds * 1_000)
        {
            return Ok(DevicePoll::SlowDown {
                interval_seconds: found.interval_seconds + 1,
            });
        }
        tx.execute(
            "UPDATE device_authorizations SET last_polled_at = ?2 WHERE device_code_hash = ?1",
            params![token_hash(device_code), now],
        )?;
        let Some(workspace) = found.approved_by else {
            tx.commit()?;
            return Ok(DevicePoll::Pending {
                interval_seconds: found.interval_seconds,
            });
        };

        let token = format!("rm_{}", uuid::Uuid::new_v4().simple());
        let machine = MachineRecord {
            id: format!("machine_{}", uuid::Uuid::new_v4().simple()),
            name: if found.machine_name.trim().is_empty() {
                "Rebeam CLI".to_string()
            } else {
                found.machine_name
            },
            created_at: now,
            last_seen: Some(now),
            online: true,
            runtimes: Vec::new(),
            runtime_updated_at: None,
        };
        tx.execute(
            "INSERT INTO machine_tokens
               (token_hash, workspace_id, created_at, last_seen, id, name)
             VALUES (?1, ?2, ?3, ?3, ?4, ?5)",
            params![token_hash(&token), workspace, now, machine.id, machine.name],
        )?;
        tx.execute(
            "UPDATE device_authorizations SET consumed_at = ?2 WHERE device_code_hash = ?1",
            params![token_hash(device_code), now],
        )?;
        tx.commit()?;
        Ok(DevicePoll::Authorized { token, machine })
    }

    pub fn machines(&self, workspace: &str) -> rusqlite::Result<Vec<MachineRecord>> {
        let conn = self.conn.lock().unwrap();
        let cutoff = now_millis() - 45_000;
        let mut statement = conn.prepare(
            "SELECT id, name, created_at, last_seen, runtime_inventory, runtime_updated_at
             FROM machine_tokens
             WHERE workspace_id = ?1 AND revoked_at IS NULL AND id IS NOT NULL
             ORDER BY created_at DESC",
        )?;
        let rows = statement
            .query_map(params![workspace], |row| {
                let last_seen: Option<i64> = row.get(3)?;
                Ok(MachineRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    last_seen,
                    online: last_seen.is_some_and(|seen| seen >= cutoff),
                    runtimes: parse_inventory(row.get::<_, String>(4)?),
                    runtime_updated_at: row.get(5)?,
                })
            })?
            .collect();
        rows
    }

    pub fn machine_for_credential(&self, token: &str) -> rusqlite::Result<Option<MachineRecord>> {
        let conn = self.conn.lock().unwrap();
        let cutoff = now_millis() - 45_000;
        conn.query_row(
            "SELECT id, name, created_at, last_seen, runtime_inventory, runtime_updated_at
             FROM machine_tokens
             WHERE token_hash = ?1 AND revoked_at IS NULL AND id IS NOT NULL",
            params![token_hash(token)],
            |row| {
                let last_seen: Option<i64> = row.get(3)?;
                Ok(MachineRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    last_seen,
                    online: last_seen.is_some_and(|seen| seen >= cutoff),
                    runtimes: parse_inventory(row.get::<_, String>(4)?),
                    runtime_updated_at: row.get(5)?,
                })
            },
        )
        .optional()
    }

    pub fn update_runtime_inventory(&self, token: &str, inventory: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE machine_tokens
             SET runtime_inventory = ?2, runtime_updated_at = ?3, last_seen = ?3
             WHERE token_hash = ?1 AND revoked_at IS NULL",
            params![token_hash(token), inventory, now_millis()],
        )? > 0)
    }

    pub fn revoke_machine(&self, workspace: &str, machine: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE machine_tokens SET revoked_at = ?3
             WHERE workspace_id = ?1 AND id = ?2 AND revoked_at IS NULL",
            params![workspace, machine, now_millis()],
        )? > 0)
    }

    pub fn revoke_machine_token(&self, token: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE machine_tokens SET revoked_at = ?2
             WHERE token_hash = ?1 AND revoked_at IS NULL",
            params![token_hash(token), now_millis()],
        )? > 0)
    }

    // -- tool approvals ---------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn create_approval(
        &self,
        owner: &str,
        machine: &str,
        agent: &str,
        chat: &str,
        run: &str,
        tool_call: &str,
        provider: &str,
        tool: &str,
        display: &ApprovalDisplay,
        input_digest: &str,
        expires_at: i64,
    ) -> rusqlite::Result<ApprovalCreate> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let existing = tx
            .query_row(
                "SELECT id, owner_id, machine_id, agent_id, chat_id, run_id, tool_call_id,
                        provider, tool, display, input_digest, state, expires_at, created_at,
                        resolved_at, resolved_by, resolution_reason
                 FROM approvals WHERE machine_id = ?1 AND run_id = ?2 AND tool_call_id = ?3",
                params![machine, run, tool_call],
                row_to_approval,
            )
            .optional()?;
        if let Some(existing) = existing {
            let same = existing.owner_id == owner
                && existing.agent_id == agent
                && existing.chat_id == chat
                && existing.provider == provider
                && existing.tool == tool
                && existing.display == *display
                && existing.input_digest == input_digest;
            return Ok(if same {
                ApprovalCreate::Existing(existing)
            } else {
                ApprovalCreate::Conflict
            });
        }
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM approvals WHERE machine_id = ?1 AND state = 'pending'",
            params![machine],
            |row| row.get(0),
        )?;
        if pending >= 100 {
            return Ok(ApprovalCreate::TooMany);
        }

        let approval = Approval {
            id: format!("apr_{}", uuid::Uuid::new_v4().simple()),
            owner_id: owner.to_owned(),
            machine_id: machine.to_owned(),
            agent_id: agent.to_owned(),
            chat_id: chat.to_owned(),
            run_id: run.to_owned(),
            tool_call_id: tool_call.to_owned(),
            provider: provider.to_owned(),
            tool: tool.to_owned(),
            display: display.clone(),
            input_digest: input_digest.to_owned(),
            state: ApprovalState::Pending,
            expires_at,
            created_at: now_millis(),
            resolved_at: None,
            resolved_by: None,
            resolution_reason: None,
        };
        tx.execute(
            "INSERT INTO approvals
               (id, owner_id, machine_id, agent_id, chat_id, run_id, tool_call_id,
                provider, tool, display, input_digest, state, expires_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'pending', ?12, ?13)",
            params![
                approval.id,
                approval.owner_id,
                approval.machine_id,
                approval.agent_id,
                approval.chat_id,
                approval.run_id,
                approval.tool_call_id,
                approval.provider,
                approval.tool,
                serde_json::to_string(&approval.display).unwrap(),
                approval.input_digest,
                approval.expires_at,
                approval.created_at,
            ],
        )?;
        append_approval_audit(
            &tx,
            &approval.id,
            "machine",
            machine,
            None,
            "pending",
            "requested",
        )?;
        tx.commit()?;
        Ok(ApprovalCreate::Created(approval))
    }

    pub fn approvals_for_owner(&self, owner: &str) -> rusqlite::Result<Vec<Approval>> {
        self.expire_approvals()?;
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT id, owner_id, machine_id, agent_id, chat_id, run_id, tool_call_id,
                    provider, tool, display, input_digest, state, expires_at, created_at,
                    resolved_at, resolved_by, resolution_reason
             FROM approvals WHERE owner_id = ?1 ORDER BY created_at",
        )?;
        let approvals = statement
            .query_map(params![owner], row_to_approval)?
            .collect();
        approvals
    }

    /// Owner-scoped audit trail: every recorded approval state transition for
    /// approvals this owner owns, newest first. Backs the audit/export view.
    pub fn approval_audit_for_owner(
        &self,
        owner: &str,
        limit: i64,
    ) -> rusqlite::Result<Vec<AuditEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT au.approval_id, ap.tool, ap.agent_id, ap.chat_id, ap.provider,
                    au.actor_type, au.actor_id, au.from_state, au.to_state, au.reason, au.created_at
             FROM approval_audit au
             JOIN approvals ap ON ap.id = au.approval_id
             WHERE ap.owner_id = ?1
             ORDER BY au.created_at DESC, au.id DESC
             LIMIT ?2",
        )?;
        let entries = statement
            .query_map(params![owner, limit], |row| {
                Ok(AuditEntry {
                    approval_id: row.get(0)?,
                    tool: row.get(1)?,
                    agent_id: row.get(2)?,
                    chat_id: row.get(3)?,
                    provider: row.get(4)?,
                    actor_type: row.get(5)?,
                    actor_id: row.get(6)?,
                    from_state: row.get(7)?,
                    to_state: row.get(8)?,
                    reason: row.get(9)?,
                    created_at: row.get(10)?,
                })
            })?
            .collect();
        entries
    }

    /// Owner-scoped operational metrics: current approval counts by state.
    pub fn approval_metrics_for_owner(&self, owner: &str) -> rusqlite::Result<ApprovalMetrics> {
        self.expire_approvals()?;
        let conn = self.conn.lock().unwrap();
        let mut statement = conn
            .prepare("SELECT state, COUNT(*) FROM approvals WHERE owner_id = ?1 GROUP BY state")?;
        let rows = statement
            .query_map(params![owner], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
        let mut metrics = ApprovalMetrics::default();
        for row in rows {
            let (state, count) = row?;
            let count = count.max(0) as u64;
            match state.as_str() {
                "pending" => metrics.pending = count,
                "allowed" => metrics.allowed = count,
                "denied" => metrics.denied = count,
                "expired" => metrics.expired = count,
                "cancelled" => metrics.cancelled = count,
                _ => {}
            }
            metrics.total += count;
        }
        Ok(metrics)
    }

    pub fn approval_for_owner(&self, id: &str, owner: &str) -> rusqlite::Result<Option<Approval>> {
        self.expire_approvals()?;
        self.approval_where("id = ?1 AND owner_id = ?2", id, owner)
    }

    pub fn approval_for_machine(
        &self,
        id: &str,
        machine: &str,
    ) -> rusqlite::Result<Option<Approval>> {
        self.expire_approvals()?;
        self.approval_where("id = ?1 AND machine_id = ?2", id, machine)
    }

    fn approval_where(
        &self,
        predicate: &str,
        id: &str,
        identity: &str,
    ) -> rusqlite::Result<Option<Approval>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            &format!(
                "SELECT id, owner_id, machine_id, agent_id, chat_id, run_id, tool_call_id,
                        provider, tool, display, input_digest, state, expires_at, created_at,
                        resolved_at, resolved_by, resolution_reason
                 FROM approvals WHERE {predicate}"
            ),
            params![id, identity],
            row_to_approval,
        )
        .optional()
    }

    pub fn resolve_approval(
        &self,
        id: &str,
        owner: &str,
        decision: ApprovalDecision,
        input_digest: &str,
    ) -> rusqlite::Result<ApprovalTransition> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let Some(current) = approval_in_transaction(&tx, id, "owner_id", owner)? else {
            return Ok(ApprovalTransition::NotFound);
        };
        if current.state != ApprovalState::Pending {
            return Ok(ApprovalTransition::NotPending);
        }
        let now = now_millis();
        if now >= current.expires_at {
            let expired = transition_approval(
                &tx,
                &current,
                ApprovalState::Expired,
                now,
                None,
                Some("expired"),
            )?;
            append_approval_audit(
                &tx,
                id,
                "system",
                "relay",
                Some("pending"),
                "expired",
                "deadline reached",
            )?;
            tx.commit()?;
            return Ok(ApprovalTransition::Expired(expired));
        }
        if current.input_digest != input_digest {
            let denied = transition_approval(
                &tx,
                &current,
                ApprovalState::Denied,
                now,
                Some(owner),
                Some("input digest mismatch"),
            )?;
            append_approval_audit(
                &tx,
                id,
                "human",
                owner,
                Some("pending"),
                "denied",
                "input digest mismatch",
            )?;
            tx.commit()?;
            return Ok(ApprovalTransition::DigestMismatch(denied));
        }
        let next = match decision {
            ApprovalDecision::AllowOnce => ApprovalState::Allowed,
            ApprovalDecision::Deny => ApprovalState::Denied,
        };
        let updated = transition_approval(&tx, &current, next, now, Some(owner), None)?;
        append_approval_audit(
            &tx,
            id,
            "human",
            owner,
            Some("pending"),
            approval_state_str(next),
            "owner decision",
        )?;
        tx.commit()?;
        Ok(ApprovalTransition::Updated(updated))
    }

    pub fn cancel_approval(
        &self,
        id: &str,
        machine: &str,
        reason: &str,
    ) -> rusqlite::Result<ApprovalTransition> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let Some(current) = approval_in_transaction(&tx, id, "machine_id", machine)? else {
            return Ok(ApprovalTransition::NotFound);
        };
        if current.state != ApprovalState::Pending {
            return Ok(ApprovalTransition::NotPending);
        }
        let now = now_millis();
        if now >= current.expires_at {
            let expired = transition_approval(
                &tx,
                &current,
                ApprovalState::Expired,
                now,
                None,
                Some("expired"),
            )?;
            append_approval_audit(
                &tx,
                id,
                "system",
                "relay",
                Some("pending"),
                "expired",
                "deadline reached",
            )?;
            tx.commit()?;
            return Ok(ApprovalTransition::Expired(expired));
        }
        let cancelled = transition_approval(
            &tx,
            &current,
            ApprovalState::Cancelled,
            now,
            Some(machine),
            Some(reason),
        )?;
        append_approval_audit(
            &tx,
            id,
            "machine",
            machine,
            Some("pending"),
            "cancelled",
            reason,
        )?;
        tx.commit()?;
        Ok(ApprovalTransition::Updated(cancelled))
    }

    pub fn cancel_approvals_for_agent_chat(
        &self,
        agent: &str,
        chat: &str,
        reason: &str,
    ) -> rusqlite::Result<Vec<Approval>> {
        self.cancel_approvals_matching("agent_id = ?1 AND chat_id = ?2", agent, chat, reason)
    }

    pub fn cancel_approvals_for_machine(
        &self,
        machine: &str,
        reason: &str,
    ) -> rusqlite::Result<Vec<Approval>> {
        self.cancel_approvals_matching(
            "machine_id = ?1 AND machine_id = ?2",
            machine,
            machine,
            reason,
        )
    }

    fn cancel_approvals_matching(
        &self,
        predicate: &str,
        first: &str,
        second: &str,
        reason: &str,
    ) -> rusqlite::Result<Vec<Approval>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let sql = format!(
            "SELECT id, owner_id, machine_id, agent_id, chat_id, run_id, tool_call_id,
                    provider, tool, display, input_digest, state, expires_at, created_at,
                    resolved_at, resolved_by, resolution_reason
             FROM approvals WHERE state = 'pending' AND {predicate}"
        );
        let pending = tx
            .prepare(&sql)?
            .query_map(params![first, second], row_to_approval)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let now = now_millis();
        let mut cancelled = Vec::with_capacity(pending.len());
        for current in pending {
            let updated = transition_approval(
                &tx,
                &current,
                ApprovalState::Cancelled,
                now,
                None,
                Some(reason),
            )?;
            append_approval_audit(
                &tx,
                &current.id,
                "system",
                "relay",
                Some("pending"),
                "cancelled",
                reason,
            )?;
            cancelled.push(updated);
        }
        tx.commit()?;
        Ok(cancelled)
    }

    pub fn expire_approvals(&self) -> rusqlite::Result<Vec<Approval>> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = now_millis();
        let pending = tx
            .prepare(
                "SELECT id, owner_id, machine_id, agent_id, chat_id, run_id, tool_call_id,
                        provider, tool, display, input_digest, state, expires_at, created_at,
                        resolved_at, resolved_by, resolution_reason
                 FROM approvals WHERE state = 'pending' AND expires_at <= ?1",
            )?
            .query_map(params![now], row_to_approval)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut expired = Vec::with_capacity(pending.len());
        for current in pending {
            let updated = transition_approval(
                &tx,
                &current,
                ApprovalState::Expired,
                now,
                None,
                Some("expired"),
            )?;
            append_approval_audit(
                &tx,
                &current.id,
                "system",
                "relay",
                Some("pending"),
                "expired",
                "deadline reached",
            )?;
            expired.push(updated);
        }
        tx.commit()?;
        Ok(expired)
    }

    fn new_session(conn: &Connection, user: User) -> rusqlite::Result<AuthSession> {
        let token = format!("rb_{}", uuid::Uuid::new_v4().simple());
        let refresh_token = format!("rr_{}", uuid::Uuid::new_v4().simple());
        let now = now_millis();
        let expires_at = now + ACCESS_TTL_MS;
        conn.execute(
            "INSERT INTO sessions
               (token_hash, user_id, created_at, expires_at, refresh_token_hash, refresh_expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                token_hash(&token),
                user.id,
                now,
                expires_at,
                token_hash(&refresh_token),
                now + REFRESH_TTL_MS
            ],
        )?;
        Ok(AuthSession {
            token,
            refresh_token,
            expires_at,
            user,
        })
    }

    /// A signed-in person is also a chat member. Keeping these IDs equal lets
    /// membership checks be the single visibility boundary for the MVP.
    fn ensure_user_member(conn: &Connection, user: &User) -> rusqlite::Result<()> {
        let position: i64 = conn.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM members",
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO members (id, name, kind, presence, position)
             VALUES (?1, ?2, 'human', 'online', ?3)",
            params![user.id, user.name, position],
        )?;
        Ok(())
    }

    // -- directory ---------------------------------------------------------

    pub fn chats(&self) -> rusqlite::Result<Vec<Chat>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id, name, kind, topic, avatar_seed FROM chats ORDER BY position")?;
        let rows = stmt.query_map([], |r| {
            Ok(Chat {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: match r.get::<_, String>(2)?.as_str() {
                    "dm" => ChatKind::Dm,
                    _ => ChatKind::Group,
                },
                member_ids: Vec::new(),
                topic: r.get(3)?,
                avatar_seed: r.get(4)?,
            })
        })?;
        let mut chats: Vec<Chat> = rows.collect::<rusqlite::Result<_>>()?;

        let mut members = conn.prepare("SELECT member_id FROM chat_members WHERE chat_id = ?1")?;
        for chat in &mut chats {
            chat.member_ids = members
                .query_map(params![chat.id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(chats)
    }

    /// Only chats the signed-in member belongs to. Seeded demo chats are
    /// therefore invisible to every new account.
    pub fn chats_for_member(&self, member: &str) -> rusqlite::Result<Vec<Chat>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT chats.id, chats.name, chats.kind, chats.topic, chats.avatar_seed
             FROM chats JOIN chat_members ON chat_members.chat_id = chats.id
             WHERE chat_members.member_id = ?1 ORDER BY chats.position",
        )?;
        let rows = stmt.query_map(params![member], |r| {
            Ok(Chat {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: if r.get::<_, String>(2)? == "dm" {
                    ChatKind::Dm
                } else {
                    ChatKind::Group
                },
                member_ids: Vec::new(),
                topic: r.get(3)?,
                avatar_seed: r.get(4)?,
            })
        })?;
        let mut chats: Vec<Chat> = rows.collect::<rusqlite::Result<_>>()?;
        let mut members = conn.prepare("SELECT member_id FROM chat_members WHERE chat_id = ?1")?;
        for chat in &mut chats {
            chat.member_ids = members
                .query_map(params![chat.id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(chats)
    }

    pub fn update_chat(
        &self,
        id: &str,
        name: Option<&str>,
        topic: Option<&str>,
        avatar_seed: Option<&str>,
    ) -> rusqlite::Result<Option<Chat>> {
        {
            let conn = self.conn.lock().unwrap();
            if let Some(name) = name {
                conn.execute(
                    "UPDATE chats SET name = ?2 WHERE id = ?1",
                    params![id, name],
                )?;
            }
            if let Some(topic) = topic {
                let topic = (!topic.is_empty()).then_some(topic);
                conn.execute(
                    "UPDATE chats SET topic = ?2 WHERE id = ?1",
                    params![id, topic],
                )?;
            }
            if let Some(avatar_seed) = avatar_seed {
                let avatar_seed = (!avatar_seed.is_empty()).then_some(avatar_seed);
                conn.execute(
                    "UPDATE chats SET avatar_seed = ?2 WHERE id = ?1",
                    params![id, avatar_seed],
                )?;
            }
        }
        Ok(self.chats()?.into_iter().find(|chat| chat.id == id))
    }

    pub fn members(&self) -> rusqlite::Result<Vec<Member>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, presence, bio, owner_id, host FROM members ORDER BY position",
        )?;
        let rows = stmt.query_map([], row_to_member)?;
        rows.collect()
    }

    pub fn members_for_member(&self, member: &str) -> rusqlite::Result<Vec<Member>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, presence, bio, owner_id, host FROM members
             WHERE id = ?1 OR id IN (
               SELECT DISTINCT other.member_id FROM chat_members AS mine
               JOIN chat_members AS other ON other.chat_id = mine.chat_id
               WHERE mine.member_id = ?1
             ) ORDER BY position",
        )?;
        let rows = stmt.query_map(params![member], row_to_member)?;
        let members = rows.collect::<rusqlite::Result<_>>()?;
        Ok(members)
    }

    pub fn is_member_of_chat(&self, chat: &str, member: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT 1 FROM chat_members WHERE chat_id = ?1 AND member_id = ?2",
                params![chat, member],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    // -- the log -----------------------------------------------------------

    pub fn messages(&self, chat: &str) -> rusqlite::Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at
             FROM messages WHERE chat_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![chat], row_to_message)?;
        rows.collect()
    }

    /// Append a message, assigning its per-chat `seq` under the same lock —
    /// which is what makes the sequence monotonic without any coordination.
    pub fn append(&self, mut msg: Message) -> rusqlite::Result<Message> {
        let conn = self.conn.lock().unwrap();
        let next: i64 = conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE chat_id = ?1",
            params![msg.channel_id],
            |r| r.get(0),
        )?;
        msg.seq = next;
        conn.execute(
            "INSERT INTO messages
               (id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                msg.id,
                msg.channel_id,
                msg.author_id,
                msg.seq,
                kind_str(msg.kind),
                msg.text,
                msg.options
                    .as_ref()
                    .map(|o| serde_json::to_string(o).unwrap()),
                msg.resolved_option,
                msg.created_at,
            ],
        )?;
        Ok(msg)
    }

    pub fn resolve(&self, id: &str, option: &str) -> rusqlite::Result<Option<Message>> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET resolved_option = ?2 WHERE id = ?1 AND resolved_option IS NULL",
            params![id, option],
        )?;
        conn.query_row(
            "SELECT id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at
             FROM messages WHERE id = ?1",
            params![id],
            row_to_message,
        )
        .optional()
    }

    pub fn message(&self, id: &str) -> rusqlite::Result<Option<Message>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at
             FROM messages WHERE id = ?1",
            params![id],
            row_to_message,
        )
        .optional()
    }

    /// Resolve a chat by id or by (case-insensitive) name — so the CLI can say
    /// `-c warroom` instead of `-c g_warroom`.
    pub fn find_chat(&self, needle: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM chats
             WHERE id = ?1 COLLATE NOCASE
                OR name = ?1 COLLATE NOCASE
                OR REPLACE(LOWER(name), ' ', '') = REPLACE(LOWER(?1), ' ', '')
             LIMIT 1",
            params![needle],
            |r| r.get(0),
        )
        .optional()
    }

    pub fn find_chat_for_member(
        &self,
        needle: &str,
        member: &str,
    ) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT chats.id FROM chats
             JOIN chat_members ON chat_members.chat_id = chats.id
             WHERE chat_members.member_id = ?2 AND (
               chats.id = ?1 COLLATE NOCASE
               OR chats.name = ?1 COLLATE NOCASE
               OR REPLACE(LOWER(chats.name), ' ', '') = REPLACE(LOWER(?1), ' ', '')
             ) LIMIT 1",
            params![needle, member],
            |r| r.get(0),
        )
        .optional()
    }

    pub fn find_member(&self, needle: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM members WHERE id = ?1 COLLATE NOCASE OR name = ?1 COLLATE NOCASE LIMIT 1",
            params![needle],
            |r| r.get(0),
        )
        .optional()
    }

    // -- membership --------------------------------------------------------

    /// The head of a chat's log. The top of every unread window.
    pub fn head_seq(&self, chat: &str) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM messages WHERE chat_id = ?1",
            params![chat],
            |r| r.get(0),
        )
    }

    /// Resolve a history grant to a seq floor, at redemption time.
    fn floor_for(conn: &Connection, chat: &str, grant: HistoryGrant) -> rusqlite::Result<i64> {
        Ok(match grant {
            HistoryGrant::All => 0,
            HistoryGrant::None => conn.query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM messages WHERE chat_id = ?1",
                params![chat],
                |r| r.get(0),
            )?,
            // The last seq strictly before the cutoff, so the first readable
            // message is the first one at or after it.
            HistoryGrant::Since { ms } => conn.query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM messages
                 WHERE chat_id = ?1 AND created_at < ?2",
                params![chat, ms],
                |r| r.get(0),
            )?,
        })
    }

    pub fn membership(&self, chat: &str, member: &str) -> rusqlite::Result<Option<Membership>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT chat_id, member_id, invited_by, joined_at, history_floor_seq, cursor_seq, trigger
             FROM chat_members WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member],
            row_to_membership,
        )
        .optional()
    }

    /// Attach an already-owned agent to a chat created by its owner. This is
    /// the local onboarding path; external guests still use redeemable invites.
    pub fn attach_owned_agent(
        &self,
        chat: &str,
        member: &str,
        owner: &str,
        now: i64,
    ) -> rusqlite::Result<Option<Membership>> {
        let conn = self.conn.lock().unwrap();
        let owned: Option<String> = conn
            .query_row(
                "SELECT id FROM members WHERE id = ?1 AND kind = 'agent' AND owner_id = ?2",
                params![member, owner],
                |row| row.get(0),
            )
            .optional()?;
        if owned.is_none() {
            return Ok(None);
        }
        let floor = Self::floor_for(&conn, chat, HistoryGrant::All)?;
        conn.execute(
            "INSERT INTO chat_members
               (chat_id, member_id, invited_by, joined_at, history_floor_seq, cursor_seq, trigger)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, 'all')
             ON CONFLICT (chat_id, member_id) DO NOTHING",
            params![chat, member, owner, now, floor],
        )?;
        Ok(Some(Membership {
            chat: chat.to_string(),
            member: member.to_string(),
            invited_by: Some(owner.to_string()),
            joined_at: now,
            history_floor_seq: floor,
            cursor_seq: floor,
            trigger: Trigger::All,
        }))
    }

    pub fn memberships_of(&self, member: &str) -> rusqlite::Result<Vec<Membership>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT chat_id, member_id, invited_by, joined_at, history_floor_seq, cursor_seq, trigger
             FROM chat_members WHERE member_id = ?1",
        )?;
        let rows = stmt.query_map(params![member], row_to_membership)?;
        let out: Vec<Membership> = rows.collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// Messages a member is allowed to see and has not seen, oldest first.
    /// Clamped to the floor, so a rewound cursor can never reach withheld
    /// history, and capped so a long backlog cannot blow up one turn.
    pub fn unread(&self, chat: &str, member: &str, limit: usize) -> rusqlite::Result<Vec<Message>> {
        let Some(m) = self.membership(chat, member)? else {
            return Ok(Vec::new());
        };
        let from = m.cursor_seq.max(m.history_floor_seq);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at
             FROM messages
             WHERE chat_id = ?1 AND seq > ?2 AND kind != 'system'
             ORDER BY seq DESC LIMIT ?3",
        )?;
        let mut rows: Vec<Message> = stmt
            .query_map(params![chat, from, limit as i64], row_to_message)?
            .collect::<rusqlite::Result<_>>()?;
        // Take the newest N, then restore reading order.
        rows.reverse();
        Ok(rows)
    }

    /// Mark everything up to `seq` as read. Never moves backwards — a late
    /// turn finishing after a newer one must not replay what it already saw.
    pub fn advance_cursor(&self, chat: &str, member: &str, seq: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chat_members SET cursor_seq = MAX(cursor_seq, ?3)
             WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member, seq],
        )?;
        Ok(())
    }

    pub fn reset_cursor(&self, chat: &str, member: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chat_members SET cursor_seq = history_floor_seq
             WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member],
        )?;
        Ok(())
    }

    pub fn set_trigger(&self, chat: &str, member: &str, trigger: Trigger) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chat_members SET trigger = ?3 WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member, trigger_str(trigger)],
        )?;
        Ok(())
    }

    /// Remove a member from a chat. The same gesture for humans and agents,
    /// and the only revocation there is.
    pub fn kick(&self, chat: &str, member: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let gone = conn.execute(
            "DELETE FROM chat_members WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member],
        )?;
        conn.execute(
            "DELETE FROM invites WHERE chat_id = ?1 AND redeemed_by = ?2",
            params![chat, member],
        )?;
        Ok(gone > 0)
    }

    /// A new group, with its creator already in it. Everyone else arrives by
    /// invite — including the agents.
    pub fn create_chat(
        &self,
        name: &str,
        topic: Option<&str>,
        creator: &str,
        now: i64,
    ) -> rusqlite::Result<Chat> {
        let conn = self.conn.lock().unwrap();
        let id = format!("g_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let position: i64 = conn.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM chats",
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO chats (id, name, kind, topic, position) VALUES (?1, ?2, 'group', ?3, ?4)",
            params![id, name, topic, position],
        )?;
        conn.execute(
            "INSERT INTO chat_members
               (chat_id, member_id, joined_at, history_floor_seq, cursor_seq, trigger)
             VALUES (?1, ?2, ?3, 0, 0, 'all')",
            params![id, creator, now],
        )?;
        Ok(Chat {
            id,
            name: name.to_string(),
            kind: ChatKind::Group,
            member_ids: vec![creator.into()],
            topic: topic.map(str::to_string),
            avatar_seed: None,
        })
    }

    // -- invites -----------------------------------------------------------

    pub fn create_invite(
        &self,
        chat: &str,
        invited_by: &str,
        history: HistoryGrant,
        now: i64,
    ) -> rusqlite::Result<Result<Invite, String>> {
        let conn = self.conn.lock().unwrap();

        // Expired codes are dead weight, not pending invites.
        conn.execute(
            "DELETE FROM invites
             WHERE chat_id = ?1 AND redeemed_by IS NULL AND expires_at <= ?2",
            params![chat, now],
        )?;

        // Inviting three agents to one group means three live codes, so the
        // cap is a runaway backstop rather than a workflow limit. The tight
        // caps in OpenClaw and Hermes guard *inbound* requests from strangers;
        // here the inviter is already inside.
        let pending: usize = conn.query_row(
            "SELECT COUNT(*) FROM invites
             WHERE chat_id = ?1 AND redeemed_by IS NULL AND expires_at > ?2",
            params![chat, now],
            |r| r.get::<_, i64>(0).map(|n| n as usize),
        )?;
        if pending >= Invite::MAX_PENDING {
            return Ok(Err(format!(
                "{} people already have open invites to this chat",
                Invite::MAX_PENDING
            )));
        }

        let invite = Invite {
            code: new_code(),
            chat: chat.to_string(),
            invited_by: invited_by.to_string(),
            history,
            expires_at: now + Invite::TTL_MS,
            redeemed_by: None,
        };
        conn.execute(
            "INSERT INTO invites (code, chat_id, invited_by, history, expires_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                rebeam_core::normalize_code(&invite.code),
                invite.chat,
                invite.invited_by,
                serde_json::to_string(&invite.history).unwrap(),
                invite.expires_at,
                now,
            ],
        )?;
        Ok(Ok(invite))
    }

    /// Who issued an invite. The joining agent's owner, and therefore where
    /// its approvals will route once it is lent to somebody else's chat.
    pub fn invite_inviter(&self, code: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT invited_by FROM invites WHERE code = ?1",
            params![rebeam_core::normalize_code(code)],
            |r| r.get(0),
        )
        .optional()
    }

    /// Exchange a code for a membership. Single-use: the same code redeemed
    /// twice fails the second time.
    pub fn redeem(
        &self,
        code: &str,
        member: &str,
        now: i64,
    ) -> rusqlite::Result<Result<Membership, String>> {
        let key = rebeam_core::normalize_code(code);
        let conn = self.conn.lock().unwrap();

        let found: Option<(String, String, String, i64, Option<String>)> = conn
            .query_row(
                "SELECT chat_id, invited_by, history, expires_at, redeemed_by
                 FROM invites WHERE code = ?1",
                params![key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;

        let Some((chat, invited_by, history, expires_at, redeemed_by)) = found else {
            return Ok(Err("no such invite".into()));
        };
        if redeemed_by.is_some() {
            return Ok(Err("that invite was already used".into()));
        }
        if now >= expires_at {
            return Ok(Err("that invite has expired".into()));
        }

        let grant: HistoryGrant = serde_json::from_str(&history).unwrap_or_default();
        let floor = Self::floor_for(&conn, &chat, grant)?;

        // Every agent hears the room. The gateway supplies the roster and
        // explicitly tells agents to stay silent when another member is being
        // addressed; per-agent loop budgets stop accidental reply storms.
        let trigger = Trigger::All;

        conn.execute(
            "UPDATE invites SET redeemed_by = ?2 WHERE code = ?1",
            params![key, member],
        )?;
        conn.execute(
            "INSERT INTO chat_members
               (chat_id, member_id, invited_by, joined_at, history_floor_seq, cursor_seq, trigger)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)
             ON CONFLICT (chat_id, member_id) DO UPDATE SET
               history_floor_seq = MIN(history_floor_seq, excluded.history_floor_seq)",
            params![chat, member, invited_by, now, floor, trigger_str(trigger)],
        )?;

        Ok(Ok(Membership {
            chat,
            member: member.to_string(),
            invited_by: Some(invited_by),
            joined_at: now,
            history_floor_seq: floor,
            cursor_seq: floor,
            trigger,
        }))
    }

    /// Mint an agent identity for a joining host, or reuse the existing one so
    /// the same agent can join a second chat without becoming a second member.
    pub fn upsert_agent(
        &self,
        name: &str,
        owner_id: &str,
        host: Option<&str>,
    ) -> rusqlite::Result<Member> {
        // The name is the mention handle, so it is normalized before it is
        // ever stored — a member called "Hermes 1" could never be triggered.
        let name = &rebeam_core::handle(name);
        let conn = self.conn.lock().unwrap();
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM members
                 WHERE name = ?1 COLLATE NOCASE AND owner_id = ?2",
                params![name, owner_id],
                |r| r.get(0),
            )
            .optional()?;

        let id = match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE members SET owner_id = ?2, host = COALESCE(?3, host), presence = 'online' WHERE id = ?1",
                    params![id, owner_id, host],
                )?;
                id
            }
            None => {
                // The id is derived from the name for readability, but the
                // two are independent: seeded members have ids that do not
                // match their names (`claude-main` is `a_claude`), so a new
                // agent's slug can collide with an id already in use.
                let base = format!("a_{}", slug(name));
                let mut id = base.clone();
                let mut n = 2;
                while conn
                    .query_row("SELECT 1 FROM members WHERE id = ?1", params![id], |_| {
                        Ok(())
                    })
                    .optional()?
                    .is_some()
                {
                    id = format!("{base}-{n}");
                    n += 1;
                }
                let position: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(position), 0) + 1 FROM members",
                    [],
                    |r| r.get(0),
                )?;
                conn.execute(
                    "INSERT INTO members (id, name, kind, presence, owner_id, host, position)
                     VALUES (?1, ?2, 'agent', 'online', ?3, ?4, ?5)",
                    params![id, name, owner_id, host, position],
                )?;
                id
            }
        };

        conn.query_row(
            "SELECT id, name, kind, presence, bio, owner_id, host FROM members WHERE id = ?1",
            params![id],
            row_to_member,
        )
    }

    fn seed(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM members", [], |r| r.get(0))?;
        if count > 0 {
            return Ok(());
        }
        conn.execute_batch(SEED)?;
        Ok(())
    }
}

fn row_to_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let options: Option<String> = r.get(6)?;
    Ok(Message {
        id: r.get(0)?,
        channel_id: r.get(1)?,
        author_id: r.get(2)?,
        seq: r.get(3)?,
        kind: match r.get::<_, String>(4)?.as_str() {
            "ask" => MessageKind::Ask,
            "system" => MessageKind::System,
            _ => MessageKind::Text,
        },
        text: r.get(5)?,
        options: options.and_then(|o| serde_json::from_str(&o).ok()),
        resolved_option: r.get(7)?,
        created_at: r.get(8)?,
    })
}

fn row_to_approval(r: &rusqlite::Row<'_>) -> rusqlite::Result<Approval> {
    let display: String = r.get(9)?;
    Ok(Approval {
        id: r.get(0)?,
        owner_id: r.get(1)?,
        machine_id: r.get(2)?,
        agent_id: r.get(3)?,
        chat_id: r.get(4)?,
        run_id: r.get(5)?,
        tool_call_id: r.get(6)?,
        provider: r.get(7)?,
        tool: r.get(8)?,
        display: serde_json::from_str(&display).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                9,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        input_digest: r.get(10)?,
        state: approval_state(r.get::<_, String>(11)?.as_str()),
        expires_at: r.get(12)?,
        created_at: r.get(13)?,
        resolved_at: r.get(14)?,
        resolved_by: r.get(15)?,
        resolution_reason: r.get(16)?,
    })
}

fn approval_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    identity_column: &str,
    identity: &str,
) -> rusqlite::Result<Option<Approval>> {
    tx.query_row(
        &format!(
            "SELECT id, owner_id, machine_id, agent_id, chat_id, run_id, tool_call_id,
                    provider, tool, display, input_digest, state, expires_at, created_at,
                    resolved_at, resolved_by, resolution_reason
             FROM approvals WHERE id = ?1 AND {identity_column} = ?2"
        ),
        params![id, identity],
        row_to_approval,
    )
    .optional()
}

fn transition_approval(
    tx: &rusqlite::Transaction<'_>,
    current: &Approval,
    next: ApprovalState,
    at: i64,
    by: Option<&str>,
    reason: Option<&str>,
) -> rusqlite::Result<Approval> {
    let changed = tx.execute(
        "UPDATE approvals
         SET state = ?2, resolved_at = ?3, resolved_by = ?4, resolution_reason = ?5
         WHERE id = ?1 AND state = 'pending'",
        params![current.id, approval_state_str(next), at, by, reason],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    let mut updated = current.clone();
    updated.state = next;
    updated.resolved_at = Some(at);
    updated.resolved_by = by.map(str::to_owned);
    updated.resolution_reason = reason.map(str::to_owned);
    Ok(updated)
}

#[allow(clippy::too_many_arguments)]
/// One recorded approval state transition, joined with its approval's tool and
/// routing context, for the owner-scoped audit/export view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub approval_id: String,
    pub tool: String,
    pub agent_id: String,
    pub chat_id: String,
    pub provider: String,
    pub actor_type: String,
    pub actor_id: String,
    pub from_state: Option<String>,
    pub to_state: String,
    pub reason: String,
    pub created_at: i64,
}

/// Owner-scoped operational metrics: current approval counts by terminal state.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApprovalMetrics {
    pub pending: u64,
    pub allowed: u64,
    pub denied: u64,
    pub expired: u64,
    pub cancelled: u64,
    pub total: u64,
}

fn append_approval_audit(
    tx: &rusqlite::Transaction<'_>,
    approval: &str,
    actor_type: &str,
    actor_id: &str,
    from_state: Option<&str>,
    to_state: &str,
    reason: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO approval_audit
           (approval_id, actor_type, actor_id, from_state, to_state, reason, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            approval,
            actor_type,
            actor_id,
            from_state,
            to_state,
            reason,
            now_millis()
        ],
    )?;
    Ok(())
}

fn approval_state(raw: &str) -> ApprovalState {
    match raw {
        "allowed" => ApprovalState::Allowed,
        "denied" => ApprovalState::Denied,
        "expired" => ApprovalState::Expired,
        "cancelled" => ApprovalState::Cancelled,
        _ => ApprovalState::Pending,
    }
}

fn approval_state_str(state: ApprovalState) -> &'static str {
    match state {
        ApprovalState::Pending => "pending",
        ApprovalState::Allowed => "allowed",
        ApprovalState::Denied => "denied",
        ApprovalState::Expired => "expired",
        ApprovalState::Cancelled => "cancelled",
    }
}

fn kind_str(kind: MessageKind) -> &'static str {
    match kind {
        MessageKind::Text => "text",
        MessageKind::Ask => "ask",
        MessageKind::System => "system",
    }
}

fn row_to_member(r: &rusqlite::Row<'_>) -> rusqlite::Result<Member> {
    Ok(Member {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: match r.get::<_, String>(2)?.as_str() {
            "human" => MemberKind::Human,
            _ => MemberKind::Agent,
        },
        presence: match r.get::<_, String>(3)?.as_str() {
            "online" => Presence::Online,
            _ => Presence::Offline,
        },
        bio: r.get(4)?,
        owner_id: r.get(5)?,
        host: r.get(6)?,
    })
}

fn row_to_membership(r: &rusqlite::Row<'_>) -> rusqlite::Result<Membership> {
    Ok(Membership {
        chat: r.get(0)?,
        member: r.get(1)?,
        invited_by: r.get(2)?,
        joined_at: r.get(3)?,
        history_floor_seq: r.get(4)?,
        cursor_seq: r.get(5)?,
        trigger: match r.get::<_, String>(6)?.as_str() {
            "all" => Trigger::All,
            "never" => Trigger::Never,
            _ => Trigger::Mention,
        },
    })
}

fn trigger_str(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Mention => "mention",
        Trigger::All => "all",
        Trigger::Never => "never",
    }
}

/// A code from the unambiguous alphabet. The alphabet is exactly 32 long, so
/// masking a random byte to 5 bits is uniform — no modulo skew.
fn new_code() -> String {
    let bytes = uuid::Uuid::new_v4();
    let chars: String = bytes.as_bytes()[..Invite::LEN]
        .iter()
        .map(|b| rebeam_core::CODE_ALPHABET[(b & 0x1f) as usize] as char)
        .collect();
    format!("RB-{}-{}", &chars[..4], &chars[4..])
}

fn slug(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn parse_inventory(encoded: String) -> Vec<serde_json::Value> {
    serde_json::from_str(&encoded).unwrap_or_default()
}

fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::Password)
}

fn verify_password(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).ok().is_some_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

fn is_constraint(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == ErrorCode::ConstraintViolation
    )
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Add the membership and identity columns to databases created before they
/// existed. SQLite has no `ADD COLUMN IF NOT EXISTS`, and re-adding one is the
/// only error we expect here, so a failure means "already present".
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    for stmt in MIGRATIONS {
        let _ = conn.execute(stmt, []);
    }
    Ok(())
}

const MIGRATIONS: &[&str] = &[
    "ALTER TABLE chat_members ADD COLUMN invited_by TEXT",
    "ALTER TABLE chat_members ADD COLUMN joined_at INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE chat_members ADD COLUMN history_floor_seq INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE chat_members ADD COLUMN cursor_seq INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE chat_members ADD COLUMN trigger TEXT NOT NULL DEFAULT 'mention'",
    "ALTER TABLE members ADD COLUMN owner_id TEXT",
    "ALTER TABLE members ADD COLUMN host TEXT",
    "ALTER TABLE machine_tokens ADD COLUMN last_seen INTEGER",
    "ALTER TABLE sessions ADD COLUMN expires_at INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE sessions ADD COLUMN refresh_token_hash TEXT",
    "ALTER TABLE sessions ADD COLUMN refresh_expires_at INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE machine_tokens ADD COLUMN id TEXT",
    "ALTER TABLE machine_tokens ADD COLUMN name TEXT NOT NULL DEFAULT 'Paired machine'",
    "ALTER TABLE machine_tokens ADD COLUMN revoked_at INTEGER",
    "ALTER TABLE machine_tokens ADD COLUMN runtime_inventory TEXT NOT NULL DEFAULT '[]'",
    "ALTER TABLE machine_tokens ADD COLUMN runtime_updated_at INTEGER",
    "UPDATE machine_tokens SET id = 'machine_' || substr(token_hash, 1, 32) WHERE id IS NULL",
    "CREATE UNIQUE INDEX IF NOT EXISTS machine_tokens_id ON machine_tokens(id) WHERE id IS NOT NULL",
    "CREATE UNIQUE INDEX IF NOT EXISTS sessions_refresh ON sessions(refresh_token_hash) WHERE refresh_token_hash IS NOT NULL",
];

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL UNIQUE COLLATE NOCASE,
  name TEXT NOT NULL,
  password_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  refresh_token_hash TEXT UNIQUE,
  refresh_expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS login_failures (
  email TEXT NOT NULL,
  failed_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS login_failures_email_time ON login_failures(email, failed_at);
CREATE TABLE IF NOT EXISTS pairings (
  code TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, expires_at INTEGER NOT NULL, redeemed_at INTEGER
);
CREATE TABLE IF NOT EXISTS machine_tokens (
  token_hash TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at INTEGER NOT NULL,
  last_seen INTEGER,
  id TEXT UNIQUE,
  name TEXT NOT NULL DEFAULT 'Paired machine',
  revoked_at INTEGER,
  runtime_inventory TEXT NOT NULL DEFAULT '[]',
  runtime_updated_at INTEGER
);
CREATE INDEX IF NOT EXISTS sessions_user ON sessions(user_id);
CREATE TABLE IF NOT EXISTS device_authorizations (
  device_code_hash TEXT PRIMARY KEY,
  user_code TEXT NOT NULL UNIQUE COLLATE NOCASE,
  machine_name TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  interval_seconds INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  approved_by TEXT REFERENCES users(id) ON DELETE CASCADE,
  approved_at INTEGER,
  consumed_at INTEGER,
  last_polled_at INTEGER
);
CREATE TABLE IF NOT EXISTS members (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
  presence TEXT NOT NULL DEFAULT 'offline', bio TEXT, position INTEGER DEFAULT 0,
  owner_id TEXT, host TEXT
);
CREATE TABLE IF NOT EXISTS chats (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
  topic TEXT, avatar_seed TEXT, position INTEGER DEFAULT 0
);
-- One member's standing in one chat. Permissions, context, and revocation all
-- hang off this row.
CREATE TABLE IF NOT EXISTS chat_members (
  chat_id TEXT NOT NULL,
  member_id TEXT NOT NULL,
  invited_by TEXT,
  joined_at INTEGER NOT NULL DEFAULT 0,
  history_floor_seq INTEGER NOT NULL DEFAULT 0,
  cursor_seq INTEGER NOT NULL DEFAULT 0,
  trigger TEXT NOT NULL DEFAULT 'mention',
  PRIMARY KEY (chat_id, member_id)
);
CREATE TABLE IF NOT EXISTS invites (
  code TEXT PRIMARY KEY,
  chat_id TEXT NOT NULL,
  invited_by TEXT NOT NULL,
  history TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  redeemed_by TEXT
);
CREATE INDEX IF NOT EXISTS invites_chat ON invites (chat_id, redeemed_by);
CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY,
  chat_id TEXT NOT NULL,
  author_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  options TEXT,
  resolved_option TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_chat_seq ON messages (chat_id, seq);
CREATE TABLE IF NOT EXISTS approvals (
  id TEXT PRIMARY KEY,
  owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  machine_id TEXT NOT NULL,
  agent_id TEXT NOT NULL REFERENCES members(id) ON DELETE CASCADE,
  chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  run_id TEXT NOT NULL,
  tool_call_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  tool TEXT NOT NULL,
  display TEXT NOT NULL,
  input_digest TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('pending', 'allowed', 'denied', 'expired', 'cancelled')),
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  resolved_at INTEGER,
  resolved_by TEXT,
  resolution_reason TEXT,
  UNIQUE (machine_id, run_id, tool_call_id)
);
CREATE INDEX IF NOT EXISTS approvals_owner_created ON approvals (owner_id, created_at);
CREATE INDEX IF NOT EXISTS approvals_machine_created ON approvals (machine_id, created_at);
CREATE INDEX IF NOT EXISTS approvals_pending_expiry ON approvals (expires_at) WHERE state = 'pending';
CREATE TABLE IF NOT EXISTS approval_audit (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  approval_id TEXT NOT NULL REFERENCES approvals(id) ON DELETE CASCADE,
  actor_type TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS approval_audit_approval ON approval_audit (approval_id, id);
"#;

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    fn temporary_database() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rebeam-auth-test-{}.db",
            uuid::Uuid::new_v4().simple()
        ))
    }

    fn remove_database(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn registered_accounts_are_distinct_and_survive_reopen() {
        let path = temporary_database();
        let (alice_id, bob_id) = {
            let store = Store::open(path.to_str().unwrap()).unwrap();
            let alice = store
                .register(" Alice@Example.com ", "Alice", "correct horse")
                .unwrap();
            let bob = store
                .register("bob@example.com", "Bob", "battery staple")
                .unwrap();
            assert_ne!(alice.user.id, bob.user.id);
            (alice.user.id, bob.user.id)
        };

        let reopened = Store::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            reopened
                .login("alice@example.com", "correct horse")
                .unwrap()
                .user
                .id,
            alice_id
        );
        assert_eq!(
            reopened
                .login("BOB@example.com", "battery staple")
                .unwrap()
                .user
                .id,
            bob_id
        );
        assert!(matches!(
            reopened.login("alice@example.com", "wrong password"),
            Err(AuthError::InvalidCredentials)
        ));
        assert!(matches!(
            reopened.register("ALICE@example.com", "Other", "another password"),
            Err(AuthError::DuplicateEmail)
        ));
        drop(reopened);
        remove_database(&path);
    }

    #[test]
    fn refreshing_rotates_both_session_credentials() {
        let store = Store::open(":memory:").unwrap();
        let first = store
            .register("refresh@example.com", "Refresh", "refresh password")
            .unwrap();
        let refreshed = store.refresh_session(&first.refresh_token).unwrap();
        assert_ne!(first.token, refreshed.token);
        assert_ne!(first.refresh_token, refreshed.refresh_token);
        assert!(store.user_for_token(&first.token).unwrap().is_none());
        assert!(store.user_for_token(&refreshed.token).unwrap().is_some());
        assert!(matches!(
            store.refresh_session(&first.refresh_token),
            Err(AuthError::InvalidCredentials)
        ));
    }

    #[test]
    fn device_authorization_is_single_use_and_revocable() {
        let store = Store::open(":memory:").unwrap();
        let owner = store
            .register("owner@example.com", "Owner", "owner password")
            .unwrap();
        let pending = store.start_device_authorization("Build Mac").unwrap();
        assert!(store
            .approve_device(&pending.user_code, &owner.user.id)
            .unwrap());

        let DevicePoll::Authorized { token, machine } = store
            .poll_device_authorization(&pending.device_code)
            .unwrap()
        else {
            panic!("approved device did not receive a credential");
        };
        assert_eq!(machine.name, "Build Mac");
        assert_eq!(
            store.machine_for_token(&token).unwrap().unwrap().id,
            owner.user.id
        );
        assert!(matches!(
            store
                .poll_device_authorization(&pending.device_code)
                .unwrap(),
            DevicePoll::Invalid
        ));
        assert!(store.revoke_machine(&owner.user.id, &machine.id).unwrap());
        assert!(store.machine_for_token(&token).unwrap().is_none());
    }

    #[test]
    fn chat_metadata_round_trips_and_can_be_cleared() {
        let store = Store::open(":memory:").unwrap();
        let workspace = store.bootstrap_workspace().unwrap().user;
        let chat = store
            .create_chat("before", None, &workspace.id, now_millis())
            .unwrap();

        let updated = store
            .update_chat(
                &chat.id,
                Some("after"),
                Some("a durable topic"),
                Some("seed-42"),
            )
            .unwrap()
            .unwrap();
        assert_eq!(updated.name, "after");
        assert_eq!(updated.topic.as_deref(), Some("a durable topic"));
        assert_eq!(updated.avatar_seed.as_deref(), Some("seed-42"));

        let cleared = store
            .update_chat(&chat.id, None, Some(""), Some(""))
            .unwrap()
            .unwrap();
        assert_eq!(cleared.topic, None);
        assert_eq!(cleared.avatar_seed, None);
    }

    #[test]
    fn approvals_are_exactly_once_audited_and_fail_closed() {
        let store = Store::open(":memory:").unwrap();
        let owner = store.bootstrap_workspace().unwrap();
        let chat = store
            .create_chat("approval-test", None, &owner.user.id, now_millis())
            .unwrap();
        let agent = store
            .upsert_agent("approval-agent", &owner.user.id, Some("test-host"))
            .unwrap();
        let invite = store
            .create_invite(&chat.id, &owner.user.id, HistoryGrant::All, now_millis())
            .unwrap()
            .unwrap();
        store
            .redeem(&invite.code, &agent.id, now_millis())
            .unwrap()
            .unwrap();
        let pairing = store.create_pairing(&owner.user.id).unwrap();
        let token = store.redeem_pairing(&pairing.code).unwrap().unwrap();
        let machine = store.machine_for_credential(&token).unwrap().unwrap();
        let display = ApprovalDisplay {
            summary: "Fake tool".into(),
            project: None,
            target: Some("fixture".into()),
            command: None,
        };

        let ApprovalCreate::Created(first) = store
            .create_approval(
                &owner.user.id,
                &machine.id,
                &agent.id,
                &chat.id,
                "run-1",
                "call-1",
                "fake",
                "FakeTool",
                &display,
                &"a".repeat(64),
                now_millis() + 60_000,
            )
            .unwrap()
        else {
            panic!("approval was not created");
        };
        assert!(matches!(
            store
                .resolve_approval(
                    &first.id,
                    "wrong-owner",
                    ApprovalDecision::AllowOnce,
                    &"a".repeat(64)
                )
                .unwrap(),
            ApprovalTransition::NotFound
        ));
        let ApprovalTransition::DigestMismatch(denied) = store
            .resolve_approval(
                &first.id,
                &owner.user.id,
                ApprovalDecision::AllowOnce,
                &"b".repeat(64),
            )
            .unwrap()
        else {
            panic!("mismatch did not fail closed");
        };
        assert_eq!(denied.state, ApprovalState::Denied);
        assert!(matches!(
            store
                .resolve_approval(
                    &first.id,
                    &owner.user.id,
                    ApprovalDecision::AllowOnce,
                    &"a".repeat(64)
                )
                .unwrap(),
            ApprovalTransition::NotPending
        ));

        let ApprovalCreate::Created(expiring) = store
            .create_approval(
                &owner.user.id,
                &machine.id,
                &agent.id,
                &chat.id,
                "run-2",
                "call-2",
                "fake",
                "FakeTool",
                &display,
                &"c".repeat(64),
                now_millis() - 1,
            )
            .unwrap()
        else {
            panic!("expiring approval was not created");
        };
        let expired = store.expire_approvals().unwrap();
        assert_eq!(expired[0].id, expiring.id);
        assert_eq!(expired[0].state, ApprovalState::Expired);

        let conn = store.conn.lock().unwrap();
        let audit_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM approval_audit", [], |row| row.get(0))
            .unwrap();
        assert_eq!(audit_rows, 4, "request + deny + request + expiry");
    }
}

const SEED: &str = r#"
INSERT INTO members (id, name, kind, presence, bio, position) VALUES
  ('u_t31k', 't31k', 'human', 'online', NULL, 0),
  ('a_claude', 'claude-main', 'agent', 'online', 'coding, deploys, general dogsbody', 1),
  ('a_kimi', 'kimi-research', 'agent', 'online', 'web research, long-doc summarization', 2),
  ('a_codex', 'codex-ci', 'agent', 'offline', 'test runs, CI babysitting', 3);

INSERT INTO chats (id, name, kind, topic, position) VALUES
  ('c_dm_claude', 'claude-main', 'dm', NULL, 0),
  ('c_dm_kimi', 'kimi-research', 'dm', NULL, 1),
  ('c_dm_codex', 'codex-ci', 'dm', NULL, 2),
  ('g_warroom', 'War Room', 'group', 'everyone, everything urgent', 3),
  ('g_shipit', 'Ship It', 'group', 'deploys and approvals', 4),
  ('g_research', 'Research Corner', 'group', 'kimi''s territory', 5);

INSERT INTO chat_members (chat_id, member_id) VALUES
  ('c_dm_claude', 'u_t31k'), ('c_dm_claude', 'a_claude'),
  ('c_dm_kimi', 'u_t31k'), ('c_dm_kimi', 'a_kimi'),
  ('c_dm_codex', 'u_t31k'), ('c_dm_codex', 'a_codex'),
  ('g_warroom', 'u_t31k'), ('g_warroom', 'a_claude'), ('g_warroom', 'a_kimi'), ('g_warroom', 'a_codex'),
  ('g_shipit', 'u_t31k'), ('g_shipit', 'a_claude'), ('g_shipit', 'a_codex'),
  ('g_research', 'u_t31k'), ('g_research', 'a_kimi');
"#;
