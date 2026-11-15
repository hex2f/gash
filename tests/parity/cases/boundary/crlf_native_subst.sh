# parity: mode=c
# parity: serial=1

v=$(MSYS_NO_PATHCONV=1 cmd.exe /d /c echo hello)
printf 'cmd subst: '
printf '%s' "$v" | od -An -c
[[ $v == hello ]] && echo "cmd equals hello: yes" || echo "cmd equals hello: no"
v=$(powershell -NoProfile -Command 'Write-Output a; Write-Output b')
printf 'ps subst: '
printf '%s' "$v" | od -An -c
n=0
while read -r l; do n=$((n + 1)); done < <(MSYS_NO_PATHCONV=1 cmd.exe /d /c "echo x& echo y")
echo "process subst lines: $n"
