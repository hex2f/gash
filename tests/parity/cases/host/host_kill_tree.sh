# parity: mode=host
# parity: serial=1
# parity: kill-after=ready

sleep 3601.$PARITY_KILL_TAG &
(
  ps1=$PARITY_PROBE_DIR/count.ps1
  echo '@(Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like ("*sleep*." + $env:PARITY_KILL_TAG + "*") }).Count' > "$ps1"
  until [ "$(powershell -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$ps1")" | tr -d '\r')" -ge 2 ] 2> /dev/null; do :; done
  : > "$PARITY_PROBE_DIR/ready"
) &
echo started
sleep 3600.$PARITY_KILL_TAG | cat
echo not-reached
