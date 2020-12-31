use crate::imports::*;
use crate::symbols::*;
use mysql::*;

pub struct Storage {
    c: mysql::PooledConn,
    tx_opts: mysql::TxOpts,
}

impl Storage {
    pub fn new(addr: &str, port: &str, user: &str, pass: &str) -> Result<Storage> {
        let pool = mysql::Pool::new(format!("mysql://{}:{}@{}:{}/", user, pass, addr, port))?;
        let mut conn = pool.get_conn()?;
        let tx_opts = mysql::TxOpts::default();
        Storage::init(&mut conn, tx_opts)?;
        Ok(Storage { c: conn, tx_opts })
    }
    fn init(conn: &mut mysql::PooledConn, tx_opts: mysql::TxOpts) -> Result<()> {
        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;
        tx.query_drop(Q_CREATE_YAP)?;
        tx.query_drop(Q_USE_YAP)?;
        tx.query_drop(Q_CREATE_TABLE_USERS)?;
        tx.query_drop(Q_CREATE_TABLE_GROUPS)?;
        tx.query_drop(Q_CREATE_TABLE_USER_MESSAGES)?;
        tx.query_drop(Q_CREATE_TABLE_GROUP_MESSAGES)?;
        tx.query_drop(Q_CREATE_TABLE_USER_READ_GROUP)?;
        tx.commit()
    }
    // TODO sanitize input
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
            let stmt_new_user = tx.prep("INSERT into u
            (email, pubkey, hashed_pass, friends, groups, status, visibility) VALUES
            (:email, :pubkey, :hashed_pass, :friends, :groups, :status, :visibility);
            ").map_err(RegisterError::DbError)?;
            tx.exec_drop(stmt_new_user, params! {
                "email" => req.email,
                "pubkey" => req.pubkey,
                "hashed_pass" => req.password_hash,
                "friends" => serde_json::to_string::<[UserId]>(&[]).unwrap(),
                "groups" => serde_json::to_string::<[UserId]>(&[]).unwrap(),
                "status" => serde_json::to_string(&UserStatus::default()).unwrap(),
                "visibility" => serde_json::to_string(&UserVisibility::default()).unwrap()
            }).map_err(RegisterError::DbError)?;
            if let Some(cid) = tx.last_insert_id() {
                tx.commit().map_err(RegisterError::DbError)?;
                Ok((cid as u32).into())
            } else {
                Err(RegisterError::Unknown)
            }
        }
    }
    // TODO sanitize input
    pub fn login(&mut self, req: LoginRequest) -> std::result::Result<UserId, LoginError> {
        debug!("storage: login request {:?}", &req);
        let mut tx = self
            .c
            .start_transaction(self.tx_opts)
            .map_err(|e| LoginError::DbError(e))?;
        match tx
            .query_first::<(u32, String), _>(format!(
                "SELECT uid, hashed_pass FROM u WHERE email = '{}';",
                &req.email
            ))
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
            },
        }
    }
    pub fn new_message_u(
        &mut self,
        sender: UserId,
        receiver: UserId,
        msg: ClientMessage,
    ) -> Option<UserMessageId> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        let stmt_new_message_u = tx.prep("INSERT INTO u_message (sender_id, receiver_id, msg_content, r)
        VALUES (:sender_id, :receiver_id, :msg_content, :r);").ok()?;
        tx.exec_drop(stmt_new_message_u, params! {
            "sender_id" => sender.to_string(),
            "receiver_id" => receiver.to_string(),
            "msg_content" => msg.to_string(),
            "r" => false
        }).ok()?;
        match tx.last_insert_id().map(UserMessageId::from) {
            Some(res) => {
                tx.commit().ok()?;
                Some(res)
            },
            None => None
        }
    }
    pub fn new_message_g(
        &mut self,
        sender: UserId,
        group: GroupId,
        msg: ClientMessage,
    ) -> Option<GroupMessageId> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        let stmt_new_message_g = tx.prep("INSERT INTO g_message (sender_id, gid, msg_content)
        VALUES (:sender_id, :gid, :msg_content);").ok()?;
        tx.exec_drop(stmt_new_message_g, params! {
            "sender_id" => sender.to_string(),
            "gid" => group.to_string(),
            "msg_content" => msg.to_string()
        }).ok()?;
        match tx.last_insert_id().map(GroupMessageId::from) {
            Some(res) => {
                tx.commit().ok()?;
                Some(res)
            },
            None => None
        }
    }
    pub fn get_user_data(&mut self, u: UserId) -> Option<UserRecord> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        unimplemented!()
    }
    pub fn get_group_data(&mut self, g: GroupId) -> Option<GroupRecord> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        unimplemented!()
    }
    pub fn get_user_user_unread(&mut self, s: UserId, r: UserId) -> Option<Vec<ServerMessage>> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        unimplemented!()
    }
    pub fn get_user_group_unread(&mut self, s: UserId, g: GroupId) -> Option<Vec<ServerMessage>> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        unimplemented!()
    }
    // TODO implement
    pub fn flag_read_u(&mut self, umid: UserMessageId) -> Option<()> {
        let mut tx = self.c.start_transaction(self.tx_opts).ok()?;
        Some(())
    }
}
