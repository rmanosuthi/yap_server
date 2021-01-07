#!/bin/bash
# Example launch json:
# {
# 	"db_addr": "mysql://dev:dev@127.0.0.1:3306",
# 	"api_addr": "127.0.0.1:8080",
# 	"ws_addr": "127.0.0.1:9999"
# }
RUST_LOG=debug cargo run -- -c 'dev_launch.json'