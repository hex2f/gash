# parity: mode=c
# parity: serial=1

read -r first <<< $'crlf\r'
printf 'herestring read: '
printf '%s' "$first" | od -An -c
while IFS= read -r l; do
  printf 'herestring loop: '
  printf '%s' "$l" | od -An -c
done <<< $'one\r\ntwo\r'
