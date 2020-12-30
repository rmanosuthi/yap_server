use crate::imports::*;
use crate::symbols::*;

#[derive(Debug)]
pub struct WebInvalidPassword;

impl Reject for WebInvalidPassword {}

#[derive(Debug)]
pub struct WebInvalidUser;

impl Reject for WebInvalidUser {}

#[derive(Debug)]
pub struct WebRegisterError {}

impl Reject for WebRegisterError {}

#[derive(Debug)]
pub struct WebChannelsError;

impl Reject for WebChannelsError {}

pub enum NetInternalError {
    ListenerBind(Box<dyn Error>),
}

#[derive(Debug)]
pub enum RegisterError {
    UserAlreadyExists,
    DbError(mysql::Error),
    Unknown,
}

#[derive(Debug)]
pub enum LoginError {
    InvalidEmail,
    InvalidPassword,
    DbError(mysql::Error),
    Unknown
}