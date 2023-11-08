#!/bin/bash

# build
cargo build --release --example peer 

tmux new-session -d -s karyons_p2p 

tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer1'\
    -l 'tcp://127.0.0.1:30000' -d  '30010'" Enter

tmux split-window -h -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer2'\
    -l 'tcp://127.0.0.1:30001' -d  '30011' -b 'tcp://127.0.0.1:30010 ' " Enter

tmux split-window -h -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer3'\
    -l 'tcp://127.0.0.1:30002' -d  '30012' -b 'tcp://127.0.0.1:30010'" Enter

tmux split-window -h -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer4'\
    -l 'tcp://127.0.0.1:30003' -d  '30013' -b 'tcp://127.0.0.1:30010'" Enter

tmux split-window -h -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer5'\
    -l 'tcp://127.0.0.1:30004' -d  '30014' -b 'tcp://127.0.0.1:30010'" Enter

tmux split-window -h -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer6'\
    -l 'tcp://127.0.0.1:30005' -d  '30015' -b 'tcp://127.0.0.1:30010'" Enter

tmux select-layout even-horizontal

sleep 3;

tmux select-pane -t karyons_p2p:0.0

tmux split-window -v -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer7'\
    -b 'tcp://127.0.0.1:30010' -b 'tcp://127.0.0.1:30011'" Enter

tmux select-pane -t karyons_p2p:0.2

tmux split-window -v -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer8'\
    -b 'tcp://127.0.0.1:30010' -b 'tcp://127.0.0.1:30012' -p 'tcp://127.0.0.1:30005'" Enter

tmux select-pane -t karyons_p2p:0.4

tmux split-window -v -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer9'\
    -b 'tcp://127.0.0.1:30010' -b 'tcp://127.0.0.1:30013'" Enter

tmux select-pane -t karyons_p2p:0.6

tmux split-window -v -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer10'\
    -b 'tcp://127.0.0.1:30010' -b 'tcp://127.0.0.1:30014'" Enter

tmux select-pane -t karyons_p2p:0.8

tmux split-window -v -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer11'\
    -b 'tcp://127.0.0.1:30010' -b 'tcp://127.0.0.1:30015'" Enter

tmux select-pane -t karyons_p2p:0.10

tmux split-window -v -t karyons_p2p
tmux send-keys -t karyons_p2p "../../target/release/examples/peer --userid 'peer12'\
    -b 'tcp://127.0.0.1:30010' -b 'tcp://127.0.0.1:30015' -b 'tcp://127.0.0.1:30011'" Enter

tmux set-window-option -t karyons_p2p synchronize-panes on

tmux attach -t karyons_p2p
