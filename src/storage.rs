use crate::imports::*;
use crate::symbols::*;
use mysql::*;

pub struct Storage {
    c: mysql::PooledConn,
    tx_opts: mysql::TxOpts,
}

impl Storage {
    /// Create a new storage container.
    /// Support may be extended to other backends in the future, but for now, only mysql is supported.
    pub fn new(sql_addr: &str) -> Result<Storage> {
        let pool = mysql::Pool::new(sql_addr)?;
        let mut conn = pool.get_conn()?;
        let tx_opts = mysql::TxOpts::default();
        Storage::init(&mut conn, tx_opts)?;
        Ok(Storage { c: conn, tx_opts })
    }
    /// Initialize the storage in case it hasn't been set up.
    /// SQL queries here include `IF NOT EXISTS` so they don't fail.
    fn init(conn: &mut mysql::PooledConn, tx_opts: mysql::TxOpts) -> Result<()> {
        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;
        tx.query_drop(Q_CREATE_YAP)?;
        tx.query_drop(Q_USE_YAP)?;
        tx.query_drop(Q_CREATE_TABLE_USERS)?;
        tx.query_drop(Q_CREATE_TABLE_GROUPS)?;
        tx.query_drop(Q_CREATE_TABLE_USER_MESSAGES)?;
        tx.query_drop(Q_CREATE_TABLE_GROUP_MESSAGES)?;
        tx.query_drop(Q_CREATE_TABLE_USER_READ_GROUP)?;
        tx.query_drop(Q_CREATE_FRIENDS)?;
        tx.commit()
    }
    /**
    Try to register a new user.
    Will fail if
    - user already exists
    - db error
    */
    pub fn try_register(
        &mut self,
        req: RegisterRequest,
    ) -> std::result::Result<UserId, RegisterError> {
        let mut tx = self
            .c
            .start_transaction(self.tx_opts)
            .map_err(RegisterError::DbError)?;
        // try to get associated userid from email
        let uids = tx
            .query::<String, _>(format!("SELECT uid FROM u WHERE email = '{}';", req.email))
            .map_err(RegisterError::DbError)?;
        if uids.len() > 0 {
            // user already exists
            Err(RegisterError::UserAlreadyExists)
        } else {
            // new user
            let stmt = tx
                .prep(
                    "INSERT into u
            (email, pubkey, hashed_pass, friends, groups, status, visibility) VALUES
            (:email, :pubkey, :hashed_pass, :friends, :groups, :status, :visibility);
            ",
                )
                .map_err(RegisterError::DbError)?;
            tx.exec_drop(
                stmt,
                params! {
                    "email" => req.email,
                    "pubkey" => req.pubkey,
                    "hashed_pass" => req.password_hash,
                    "friends" => serde_json::to_string::<[UserId]>(&[]).unwrap(),
                    "groups" => serde_json::to_string::<[UserId]>(&[]).unwrap(),
                    "status" => serde_json::to_string(&UserStatus::default()).unwrap(),
                    "visibility" => serde_json::to_string(&UserVisibility::default()).unwrap()
                },
            )
            .map_err(RegisterError::DbError)?;
            if let Some(cid) = tx.last_insert_id() {
                tx.commit().map_err(RegisterError::DbError)?;
                Ok((cid as u32).into())
            } else {
                Err(RegisterError::Unknown)
            }
        }
    }
    /**
    Try to login.
    Will fail if
    - invalid password
    - invalid email
    - db error
    */
    pub fn try_login(&mut self, req: LoginRequest) -> std::result::Result<UserId, LoginError> {
        debug!("storage: login request {:?}", &req);
        let mut tx = self
            .c
            .start_transaction(self.tx_opts)
            .map_err(|e| LoginError::DbError(e))?;
        let stmt = tx
            .prep("SELECT uid, hashed_pass FROM u WHERE email = :email;")
            .map_err(LoginError::DbError)?;

        match tx
            .exec_first::<(u32, String), _, _>(
                stmt,
                params! {
                    "email" => &req.email
                },
            )
            .map_err(LoginError::DbError)?
        {
            Some((ref_uid, ref_pass)) => {
                debug!("storage: login uid pass found");
                if ref_pass == req.password_hash {
                    tx.commit().map_err(LoginError::DbError)?;
                    Ok(UserId::from(ref_uid))
                } else {
                    Err(LoginError::InvalidPassword)
                }
            }
            None => {
                debug!("storage: login unknown email");
                Err(LoginError::InvalidEmail)
            }
        }
    }
    /**
    Post a new message destined for a user. **Does not flag message as read.**
    */
    pub fn new_message_u(
        &mut self,
        sender: UserId,
        receiver: UserId,
        msg: ClientMessage,
    ) -> Option<PublicUserMessage> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        let stmt = tx
            .prep(
                "INSERT INTO u_message (sender_id, receiver_id, msg_content, time_posted, r)
        VALUES (:sender_id, :receiver_id, :msg_content, :time_posted, :r);",
            )
            .ok()?;
        tx.exec_drop(
            stmt,
            params! {
                "sender_id" => sender.to_string(),
                "receiver_id" => receiver.to_string(),
                "msg_content" => msg.to_string(),
                "time_posted" => DateTime::<Utc>::from(SystemTime::now()).naive_utc(),
                "r" => false
            },
        )
        .ok()?;
        match tx.last_insert_id().map(UserMessageId::from) {
            Some(res) => {
                let stmt = tx.prep("SELECT * FROM u_message WHERE umid = :umid").ok()?;
                let echo_msg = tx
                    .exec_first(stmt, params! {"umid" => res.to_string()})
                    .ok()?
                    .map(PublicUserMessage::from_sql_tup)
                    .flatten()?;
                tx.commit().ok()?;
                Some(echo_msg)
            }
            None => None,
        }
    }
    /**
    Post a new message destined for a group. **Does not flag message as read.**
    */
    pub fn new_message_g(
        &mut self,
        sender: UserId,
        group: GroupId,
        msg: ClientMessage,
    ) -> Option<GroupMessageId> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        let stmt = tx
            .prep(
                "INSERT INTO g_message (sender_id, gid, msg_content)
        VALUES (:sender_id, :gid, :msg_content);",
            )
            .ok()?;
        tx.exec_drop(
            stmt,
            params! {
                "sender_id" => sender.to_string(),
                "gid" => group.to_string(),
                "msg_content" => msg.to_string()
            },
        )
        .ok()?;
        match tx.last_insert_id().map(GroupMessageId::from) {
            Some(res) => {
                tx.commit().ok()?;
                Some(res)
            }
            None => None,
        }
    }
    /**
    Get a user profile. Returns a `Serialize` public-facing version.
    */
    pub fn get_user_data(
        &mut self,
        u: UserId,
        requester: Option<UserId>,
    ) -> Option<PublicUserRecord> {
        let are_friends = self.are_friends(u.clone(), requester.clone())?;
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        let stmt = tx.prep("SELECT * FROM u WHERE uid = :uid;").ok()?;
        match tx
            .exec_first::<SqlUserRecord, _, _>(
                stmt,
                params! {
                    "uid" => {
                        let tmp: u32 = u.clone().into();
                        tmp
                    }
                },
            )
            .ok()?
        {
            Some(res) => UserRecord::from_sql_tup(res).map(|ur| {
                let u_visibility = ur.visibility.clone();
                let mask_lvl = if let Some(r) = requester {
                    if u == r {
                        // self is requesting access
                        UserMaskLevel::SelfUse
                    } else {
                        // maybe a friend is requesting?
                        if are_friends {
                            UserMaskLevel::HidePass
                        } else {
                            // treat as public
                            match u_visibility {
                                UserVisibility::Private => UserMaskLevel::HidePassEmailMembership,
                                _ => UserMaskLevel::HidePassEmail,
                            }
                        }
                    }
                } else {
                    // public access
                    match u_visibility {
                        UserVisibility::Private => UserMaskLevel::HidePassEmailMembership,
                        _ => UserMaskLevel::HidePassEmail,
                    }
                };
                ur.mask(mask_lvl)
            }),
            None => None,
        }
    }
    /// Get a group profile.
    pub fn get_group_data(&mut self, g: GroupId) -> Option<GroupRecord> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        todo!()
    }
    /**
    Get unread messages a user hasn't read *from a user*.
    Primary purpose is for the client to catch up.
    **Will not flag messages as read.**
    */
    pub fn get_user_user_unread(&mut self, s: UserId, r: UserId) -> Option<Vec<PublicUserMessage>> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        let stmt = tx
            .prep(
                "SELECT * FROM u_message WHERE
        sender_id = :sender_id AND
        receiver_id = :receiver_id AND
        r = 'false';",
            )
            .ok()?;
        let res = tx
            .exec_iter(
                stmt,
                params! {
                    "sender_id" => s.into_sql(),
                    "receiver_id" => r.into_sql()
                },
            )
            .ok()?
            .filter_map(Result::ok)
            .map(from_row)
            .map(PublicUserMessage::from_sql_tup)
            .collect();
        res
    }
    /**
    Get unread messages a user hasn't read *from a group*.
    Primary purpose is for the client to catch up.
    **Will not flag messages as read.**
    */
    pub fn get_user_group_unread(
        &mut self,
        s: UserId,
        g: GroupId,
    ) -> Option<Vec<PublicUserMessage>> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        todo!()
    }
    /// Flag a user message as read.
    pub fn flag_u_read(&mut self, umid: UserMessageId) -> Option<()> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        let stmt = tx
            .prep("UPDATE u_message SET r = 'true' WHERE umid = :umid;")
            .ok()?;
        tx.exec_drop(
            stmt,
            params! {
                "umid" => umid.into_sql()
            },
        )
        .ok()
    }
    /// Flag a group message as read.
    pub fn flag_g_read(&mut self, gmid: GroupMessageId) -> Option<()> {
        todo!()
    }
    /// Check if two users are friends.
    /// Assumes `(l, r)` and `(r, l)` were added on entry.
    pub fn are_friends(&mut self, l: UserId, r: Option<UserId>) -> Option<bool> {
        match r {
            Some(r) => {
                let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
                let stmt = tx.prep("SELECT (r) FROM u_friend WHERE l = :l;").ok()?;
                let it = tx
                    .exec_iter(
                        stmt,
                        params! {
                            "l" => l.into_sql()
                        },
                    )
                    .ok()?;
                let r: u32 = r.into();
                // necessary for lifetime reasons
                let res = it
                    .filter_map(Result::ok)
                    .map(from_row::<u32>)
                    .any(|r_db| r == r_db);
                Some(res)
            }
            None => Some(false),
        }
    }
    /**
    Add two users as friends.
    No manual validation of whether the uids are valid is done, as in the database should handle it because of foreign key relations.
    */
    pub fn add_friend(&mut self, l: UserId, r: UserId) -> Option<()> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        let stmt = tx.prep("INSERT INTO u_friend (l, r) VALUES (:l, :r); INSERT INTO u_friend (l, r) VALUES (:r, :l);").ok()?;
        tx.exec_drop(
            stmt,
            params! {
                "l" => {let tmp: u32 = l.into(); tmp},
                "r" => {let tmp: u32 = r.into(); tmp}
            },
        )
        .ok()
    }
    /**
    Remove a friend pairing.
    No manual validation of whether the uids are valid is done, as in the database should handle it because of foreign key relations.
    */
    pub fn remove_friend(&mut self, l: UserId, r: UserId) -> Option<()> {
        todo!()
    }
}
