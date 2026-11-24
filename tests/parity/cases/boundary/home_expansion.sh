# parity: mode=c
# parity: serial=1

echo "tilde: ~"
echo "tilde slash: ~/x"
echo "HOME: $HOME"
x=~/y
echo "assign: $x"
cd ~ && echo "cd ~: $(pwd)"
echo "msys env HOME: $(env | grep '^HOME=')"
echo "cmd HOME: $(MSYS_NO_PATHCONV=1 cmd.exe /d /c echo %HOME% | tr -d '\r')"
MSYS_NO_PATHCONV=1 cmd.exe /d /c "echo cmd-wrote> %HOME%\p_home_cmd.txt"
echo "builtin read: $(tr -d '\r' < ~/p_home_cmd.txt 2>&1)"
