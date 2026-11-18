# hn status, sensors, values: the first minute with the CLI
Output docs/demo/quickstart.gif

Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 620
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

Hide
Type "export HYDRONODE_PROFILE=int && clear"
Enter
Show

Type "hn status"
Sleep 300ms
Enter
Sleep 2.5s

Type "hn sensors"
Sleep 300ms
Enter
Sleep 2.5s

Type "hn values $HN_DEMO_SENSOR/temperature --from 7d"
Sleep 300ms
Enter
Sleep 3.5s
