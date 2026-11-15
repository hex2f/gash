# parity: mode=host

(
  until [ -e "$PARITY_PROBE_DIR/go" ]; do sleep 0.1; done
  echo job-output
  : > "$PARITY_PROBE_DIR/done"
  exit 4
) &
job=$!
echo before-go
: > "$PARITY_PROBE_DIR/go"
until [ -e "$PARITY_PROBE_DIR/done" ]; do sleep 0.1; done
echo after-done
wait "$job"
echo "job rc=$?"
