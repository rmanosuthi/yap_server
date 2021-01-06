# yap_server

Server for `yap` - a proof-of-concept chat program

# IMPORTANT: PROOF OF CONCEPT

**NOT FOR PRODUCTION USE.** Only basic features have been implemented. No connection encryption yet. DO NOT USE.

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

# Requirements

- Nightly Rust (`1.50` as of time of writing)
- MySQL-compatible database

# Implemented

- Login
- Register
- Direct messaging

# TODO

- Group messaging
- Connection encryption
- Message encryption
- Docs
- Testing