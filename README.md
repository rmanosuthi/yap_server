# yap_server

Server for `yap` - a proof-of-concept chat program

## IMPORTANT: PROOF OF CONCEPT

**NOT FOR PRODUCTION USE.** Only basic features have been implemented. DO NOT USE.

*For the client, go [here.](https://github.com/rmanosuthi/yap_client)*

# Highlights

- Asynchronous code and usage of multithreading
- Delegation of work to separate components ("Actor Model") using message passing for communication
- MySQL as backing database without an ORM
- Security considerations
    - Direct messages are encrypted end-to-end
    - MySQL statements are prepared
    - *(not implemented yet as self-signed certificates are a hassle and unnecessary for testing locally)*
       Connections can be trivially encrypted using HTTPS and WSS
- Usage of strong type system to ensure correctness
    - Input parsing
    - Conversion from privileged to unprivileged data types
    - No type confusion
    - Certain functional practices used, such as chaining map, filter, omitting parameters using map, to show transformation of data clearly

| Implemented | Feature | Notes |
|-------------|---------|-------|
|Done|Login
|Done|Register
|Done|Public profile
|Done|Direct messages
|Done|Password hashing
|**Done**|E2E encryption (DM)
|WIP|Group messages
|Not started|E2E encryption (Group)
|WIP|Query
|WIP|Friends

# Requirements

- Nightly Rust (`1.50` as of time of writing)
- MySQL-compatible database

# How to use

- Generate a launch config file:

```json
{
    "db_addr": "mysql://<user>:<password>@<ip>:<port>",
    "api_addr": "<ip>:<port>",
    "ws_addr": "<ip>:<port>"
}
```

- Run `yap_server -c <cfg-path>`. Optionally, `RUST_LOG=debug yap_server -c <cfg-path>` to see more.
