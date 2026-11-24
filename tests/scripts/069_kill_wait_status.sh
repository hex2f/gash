# wait reports 128+signal for a job stopped with kill, as bash does.
# Under load Git Bash reports status 0 for a job killed while it starts.
sleep 30 & p=$!; sleep 0.3; kill $p; wait $p; echo "TERM: $?"
sleep 30 & p=$!; sleep 0.3; kill -9 $p; wait $p 2>/dev/null; echo "KILL: $?"
sleep 30 & p=$!; sleep 0.3; kill -s HUP $p; wait $p; echo "HUP: $?"
sleep 30 & sleep 0.3; kill %1; wait %1; echo "job spec: $?"
ping -n 30 127.0.0.1 > /dev/null & p=$!; sleep 0.3; kill $p; wait $p; echo "native program: $?"
