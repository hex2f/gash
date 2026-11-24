# parity: mode=c
# parity: serial=1
# parity: fixture=native

echo "quoted glob chars:"
env printf '[%s]\n' '*' '*.txt' '?.txt' '[ab].txt' 'x*y' '{a,b}'
echo "unquoted glob:"
env printf '[%s]\n' *.txt
echo "star inside a spaced arg:"
env printf '[%s]\n' 'a * b' "* .txt"
echo "quoted tilde:"
env printf '[%s]\n' '~' '~/x'
