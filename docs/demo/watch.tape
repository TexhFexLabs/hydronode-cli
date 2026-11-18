# Live values in the full-screen view
Output docs/demo/watch.gif

Set Shell "bash"
Set FontSize 15
Set Width 1100
Set Height 520
Set Padding 24
Set Theme "Catppuccin Mocha"

Hide
Type "export HYDRONODE_PROFILE=${HN_DEMO_PROFILE:-demo} PS1='\n$ ' PATH=${HN_DEMO_BIN:+$HN_DEMO_BIN:}$PATH && clear"
Enter
Show

Type "hydronode watch"
Enter
Sleep 12s
Type "q"
Sleep 800ms
