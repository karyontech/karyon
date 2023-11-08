#!/bin/bash

# build
cargo build --release --example chat 

tmux new-session -d -s karyons_chat 

tmux send-keys -t karyons_chat "../../target/release/examples/chat --username 'user1'\
    -l 'tcp://127.0.0.1:40000' -d  '40010'" Enter

tmux split-window -h -t karyons_chat
tmux send-keys -t karyons_chat "../../target/release/examples/chat --username 'user2'\
    -l 'tcp://127.0.0.1:40001' -d  '40011' -b 'tcp://127.0.0.1:40010 ' " Enter

tmux split-window -h -t karyons_chat
tmux send-keys -t karyons_chat "../../target/release/examples/chat --username 'user3'\
    -l 'tcp://127.0.0.1:40002' -d  '40012' -b 'tcp://127.0.0.1:40010'" Enter

tmux split-window -h -t karyons_chat
tmux send-keys -t karyons_chat "../../target/release/examples/chat --username 'user4'\
    -b 'tcp://127.0.0.1:40010'" Enter

tmux select-layout tiled 

tmux attach -t karyons_chat
