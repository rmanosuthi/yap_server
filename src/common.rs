mod imports {
    pub use crate::imports::*;
    pub use crate::symbols::*;
}
pub use self::shared::*;

mod shared {
    use std::{convert::TryFrom, num::ParseIntError, str::FromStr};

    use super::imports::*;

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
        u64,            // umid
        u32,            // sender_id
        u32,            // receiver_id
        String,         // msg_content
        NaiveDateTime,  // time_posted stored as UTC
        bool            // r
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

    pub trait FromSqlTup<T> where Self: Sized {
        fn from_sql_tup(tup: T) -> Option<Self>;
    }

    /// How much information should be omitted when sending `UserRecord` to the client.
    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum UserMaskLevel {
        /// Allow everything through, **including the hashed password!**
        SelfUse,
        /// Allow everything except the hashed password.
        HidePass,
        /// Allow everything except the hashed password and email.
        HidePassEmail,
        /// Allow everything except hashed password, email, friends, and groups.
        HidePassEmailMembership,
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
                    UserMaskLevel::SelfUse => match self.status {UserStatus::Offline => false, _ => true},
                    _ => match self.status {
                        UserStatus::Online => true,
                        _ => false
                    }
                }
            }
        }
    }

    /// Public-facing version of `UserRecord`. Can be safely sent to the client.
    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct PublicUserRecord {
        pub uid: UserId,
        pub email: Option<String>,
        pub pubkey: Pubkey,
        pub hashed_pass: Option<HashedPassword>,
        pub alias: Option<String>,
        pub friends: Option<Vec<UserId>>,
        pub groups: Option<Vec<GroupId>>,
        pub motd: Option<String>,
        pub online: bool,
    }

    impl FromSqlTup<SqlUserMessage> for PublicUserMessage {
        fn from_sql_tup(tup: SqlUserMessage) -> Option<Self> {
            Some(Self {
                umid: UserMessageId::from(tup.0),
                from: UserId::from(tup.1),
                to: UserId::from(tup.2),
                content: ClientMessage::from(tup.3),
                time_posted: DateTime::from_utc(tup.4, Utc)
            })
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct PublicUserMessage {
        umid: UserMessageId,
        from: UserId,
        to: UserId,
        time_posted: DateTime<Utc>,
        content: ClientMessage
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct GroupRecord {
        pub gid: GroupId,
        pub motd: Option<String>,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum UserStatus {
        Online,
        Offline,
        Invisible,
    }

    impl Default for UserStatus {
        fn default() -> Self {
            Self::Offline
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum UserVisibility {
        Private,
        FriendsOnly,
        Public,
    }

    impl Default for UserVisibility {
        fn default() -> Self {
            Self::FriendsOnly
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, Copy)]
    pub struct UserId(u32);

    impl FromStr for UserId {
        type Err = ParseIntError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let tmp = u32::from_str(s)?;
            Ok(Self(tmp))
        }
    }

    impl Display for UserId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    pub trait IntoSqlValue where Self: Sized + Into<mysql::Value> {
        fn into_sql(self) -> mysql::Value {
            self.into()
        }
    }

    impl<T> IntoSqlValue for T where T: Sized + Into<mysql::Value> {}

    impl Into<mysql::Value> for UserId {
        fn into(self) -> mysql::Value {
            self.0.into()
        }
    }

    impl From<u32> for UserId {
        fn from(i: u32) -> Self {
            UserId(i)
        }
    }

    impl Into<u32> for UserId {
        fn into(self) -> u32 {
            self.0
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct GroupId(u32);

    impl Display for GroupId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl From<u32> for GroupId {
        fn from(i: u32) -> Self {
            GroupId(i)
        }
    }

    impl Into<mysql::Value> for GroupId {
        fn into(self) -> mysql::Value {
            self.0.into()
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct UserMessageId(u64);

    impl Display for UserMessageId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl From<u64> for UserMessageId {
        fn from(i: u64) -> Self {
            UserMessageId(i)
        }
    }

    impl Into<mysql::Value> for UserMessageId {
        fn into(self) -> mysql::Value {
            self.0.into()
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct GroupMessageId(u64);

    impl Display for GroupMessageId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl From<u64> for GroupMessageId {
        fn from(i: u64) -> Self {
            GroupMessageId(i)
        }
    }

    impl Into<mysql::Value> for GroupMessageId {
        fn into(self) -> mysql::Value {
            self.0.into()
        }
    }

    /// Public key for message signing. Server performs no validation!
    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct Pubkey(String);

    impl From<String> for Pubkey {
        fn from(s: String) -> Self {
            Pubkey(s)
        }
    }

    /*impl TryFrom<&str> for Pubkey {
        type Error = usize; // length

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            if value
                .chars()
                .all(|c| char::is_alphanumeric(c) && char::is_ascii(&c) && !char::is_whitespace(c))
                && value.chars().count() == 256
            {
                Ok(Pubkey(value.to_owned()))
            } else {
                Err(value.len())
            }
        }
    }*/

    /// Hashed password. Server performs no validation!
    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct HashedPassword(String);

    impl From<String> for HashedPassword {
        fn from(s: String) -> Self {
            HashedPassword(s)
        }
    }

    /*impl TryFrom<&str> for HashedPassword {
        type Error = usize; // length

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            if value
                .chars()
                .all(|c| char::is_alphanumeric(c) && char::is_ascii(&c) && !char::is_whitespace(c))
                && value.chars().count() == 16
            {
                Ok(HashedPassword(value.to_owned()))
            } else {
                Err(value.len())
            }
        }
    }*/

    pub const LT_LEN: usize = 40;

    pub fn alphanumeric_len(value: &str, len: usize) -> bool {
        value
            .chars()
            .all(|c| char::is_alphanumeric(c) && char::is_ascii(&c) && !char::is_whitespace(c))
            && value.chars().count() == len
    }

    #[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Debug)]
    pub struct LoginToken {
        pub tk: String,
    }

    impl LoginToken {
        pub fn new() -> LoginToken {
            LoginToken {
                tk: rand::thread_rng()
                    .sample_iter(&rand::distributions::Alphanumeric)
                    .take(LT_LEN)
                    .map(char::from)
                    .collect(),
            }
        }
    }

    impl FromStr for LoginToken {
        type Err = usize;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            if alphanumeric_len(s, LT_LEN) {
                Ok(LoginToken { tk: s.to_owned() })
            } else {
                Err(s.chars().count())
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub enum MessageActor {
        Dm(UserId),
        Group(GroupId),
    }

    #[derive(Serialize, Deserialize)]
    pub enum HistoryQuery {
        Unseen,
        Interval {
            from: DateTime<Utc>,
            to: DateTime<Utc>,
        },
        Since(DateTime<Utc>),
    }
    /** Message that is sent to the ws worker.*/
    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct WsClientboundTx {
        inner: WsClientboundPayload,
    }

    impl From<WsClientboundPayload> for WsClientboundTx {
        fn from(pl: WsClientboundPayload) -> Self {
            Self {inner: pl}
        }
    }

    impl WsClientboundTx {
        pub fn extract(self) -> Option<tungstenite::Message> {
            serde_json::to_string(&self.inner).ok().map(tungstenite::Message::Text)
        }
    }

    impl<T> From<T> for WsClientboundPayload where T: ClientboundPayload {
        fn from(i: T) -> Self {
            i.make_payload()
        }
    }
    /** Message that will be sent over ws to the client.
    The server shouldn't interfere with this, nor should it be processed in any way.
    */
    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum WsClientboundPayload {
        NewMessage(PublicUserMessage),
        NewMessages(Vec<PublicUserMessage>),
        MessageSent(UserMessageId)
    }
    #[derive(Serialize, Deserialize, Debug)]
    pub struct RegisterRequest {
        pub email: String,
        pub password_hash: String,
        pub pubkey: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct LoginRequest {
        pub email: String,
        pub password_hash: String,
    }
    pub trait ClientboundPayload where Self: Sized {
        fn make_payload(self) -> WsClientboundPayload;
    }

    impl ClientboundPayload for PublicUserMessage {
        fn make_payload(self) -> WsClientboundPayload {
            WsClientboundPayload::NewMessage(self)
        }
    }

    impl ClientboundPayload for Vec<PublicUserMessage> {
        fn make_payload(self) -> WsClientboundPayload {
            WsClientboundPayload::NewMessages(self)
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub enum WsServerboundPayload {
        NewUserMessage {
            to: UserId,
            content: ClientMessage
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct ClientMessage(String);

    impl From<String> for ClientMessage {
        fn from(s: String) -> Self {
            ClientMessage(s)
        }
    }

    impl Display for ClientMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
        }
    }
}
