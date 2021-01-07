use crate::imports::*;
use crate::symbols::*;

/// Tuple type for `UserRecord`. Only used in conversion from raw sql row.
pub type SqlUserRecord = (
    u32,            // uid
    String,         // email
    String,         // pubkey
    String,         // hashed_pass
    Option<String>, // alias
    String,         // friends
    String,         // groups
    Option<String>, // motd
    String,         // status
    String,         // visibility
);

/// Tuple type for `UserMessage`.
pub type SqlUserMessage = (
    u64,           // umid
    u32,           // sender_id
    u32,           // receiver_id
    String,        // msg_content
    NaiveDateTime, // time_posted stored as UTC
    bool,          // r
);

/// **Internal use:** User record. Intentionally made not serializable, so it doesn't accidentally get sent to the client.
#[derive(Debug, Clone)]
pub struct UserRecord {
    pub uid: UserId,
    pub email: String,
    pub pubkey: Pubkey,
    pub hashed_pass: HashedPassword,
    pub alias: Option<String>,
    pub friends: Vec<UserId>,
    pub groups: Vec<GroupId>,
    pub motd: Option<String>,
    pub status: UserStatus,
    pub visibility: UserVisibility,
}

pub trait FromSqlTup<T>
where
    Self: Sized,
{
    fn from_sql_tup(tup: T) -> Option<Self>;
}

impl FromSqlTup<SqlUserRecord> for UserRecord {
    /// Try to convert the raw sql row representation to a Rust struct.
    fn from_sql_tup(tup: SqlUserRecord) -> Option<Self> {
        let uid = UserId::from(tup.0);
        let email = tup.1;
        let pubkey = Pubkey::from(tup.2.clone());
        let hashed_pass = HashedPassword::from(tup.3.clone());
        let alias = tup.4;
        let friends = serde_json::from_str(&tup.5).ok()?;
        let groups = serde_json::from_str(&tup.6).ok()?;
        let motd = tup.7;
        let status = serde_json::from_str(&tup.8).ok()?;
        let visibility = serde_json::from_str(&tup.9).ok()?;
        Some(Self {
            uid,
            email,
            pubkey,
            hashed_pass,
            alias,
            friends,
            groups,
            motd,
            status,
            visibility,
        })
    }
}

impl UserRecord {
    /// Convert to a public-facing form with optional hiding of information.
    pub fn mask(self, mask: UserMaskLevel) -> PublicUserRecord {
        PublicUserRecord {
            uid: self.uid,
            email: match mask {
                UserMaskLevel::HidePassEmail | UserMaskLevel::HidePassEmailMembership => None,
                _ => Some(self.email),
            },
            pubkey: self.pubkey,
            hashed_pass: match mask {
                UserMaskLevel::SelfUse => Some(self.hashed_pass),
                _ => None,
            },
            alias: self.alias,
            friends: match mask {
                UserMaskLevel::HidePassEmailMembership => None,
                _ => Some(self.friends),
            },
            groups: match mask {
                UserMaskLevel::HidePassEmailMembership => None,
                _ => Some(self.groups),
            },
            motd: self.motd,
            online: match mask {
                UserMaskLevel::SelfUse => match self.status {
                    UserStatus::Offline => false,
                    _ => true,
                },
                _ => match self.status {
                    UserStatus::Online => true,
                    _ => false,
                },
            },
        }
    }
}

pub trait IntoSqlValue
where
    Self: Sized + Into<mysql::Value>,
{
    fn into_sql(self) -> mysql::Value {
        self.into()
    }
}

impl<T> IntoSqlValue for T where T: Sized + Into<mysql::Value> {}

/** Message that is sent to the ws worker.*/
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WsClientboundTx {
    inner: WsClientboundPayload,
}

impl From<WsClientboundPayload> for WsClientboundTx {
    fn from(pl: WsClientboundPayload) -> Self {
        Self { inner: pl }
    }
}

impl WsClientboundTx {
    pub fn extract(self) -> Option<tungstenite::Message> {
        serde_json::to_string(&self.inner)
            .ok()
            .map(tungstenite::Message::Text)
    }
}

#[derive(Debug)]
pub struct WsServerboundTx {
    sender: UserId,
    inner: WsServerboundPayload,
}

impl WsServerboundTx {
    /// Convert a raw ws message to a transmission.
    ///
    /// Deserialization should be done at entry point for performance reasons.
    pub fn new(sender: UserId, payload: tungstenite::Message) -> Option<WsServerboundTx> {
        match payload {
            tungstenite::Message::Text(s) => {
                serde_json::from_str(&s)
                    .ok()
                    .map(|deserialized| WsServerboundTx {
                        sender,
                        inner: deserialized,
                    })
            }
            _ => None,
        }
    }
    pub fn extract(self) -> (UserId, WsServerboundPayload) {
        (self.sender, self.inner)
    }
}
