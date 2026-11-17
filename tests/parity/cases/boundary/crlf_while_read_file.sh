# parity: mode=c
# parity: serial=1

printf 'one\r\ntwo\r\nlast' > crlf.txt
while read -r l || [ -n "$l" ]; do
  printf 'file line: '
  printf '%s' "$l" | od -An -c
done < crlf.txt
echo "msys wc: $(wc -c < crlf.txt)"
