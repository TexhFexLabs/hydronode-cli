# A fake device sending signed values, watched in the console
Output docs/demo/simulate.gif

Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 520
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

Hide
Type "export HYDRONODE_PROFILE=int && clear"
Enter
Show

Type "hn simulate $HN_DEMO_SENSOR --kind greenhouse --every 10s --count 3"
Sleep 300ms
Enter
Sleep 23s

Type "hn console $HN_DEMO_SENSOR -n 5"
Sleep 300ms
Enter
Sleep 3s
