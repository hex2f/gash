# parity: mode=c
# parity: serial=1

echo "msys child sees:"
env printf '[%s]\n' /quiet /c/x --opt=/c/x /a:/b /tmp/y x=/c/y //double
