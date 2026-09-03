#!/bin/sh
while true; do
    RUSH_SHELL=/bin/sh /home/ubuntu/rush/target/x86_64-unknown-linux-musl/release/rush -s -p 8300 >> /home/ubuntu/rush-server.log 2>&1
    echo "[watcher] server exited, restarting in 2s" >> /home/ubuntu/rush-server.log
    sleep 2
done
