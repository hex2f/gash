# parity: mode=c
# parity: serial=1

env printf '[%s]\n' 'héllo' 'ü' '日本語' 'emoji 😀' | od -An -tx1
echo "msys echo: $(env echo 'grüße')"
