# hydronode status, sensors, values: the first minute with the CLI
Output docs/demo/quickstart.gif

Set Shell "bash"
Set FontSize 15
Set Width 1500
Set Height 660
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

Hide
Type "export HYDRONODE_PROFILE=${HN_DEMO_PROFILE:-demo} PS1='\n$ ' PATH=${HN_DEMO_BIN:+$HN_DEMO_BIN:}$PATH && clear"
Enter
Show

Type "hydronode status"
Sleep 300ms
Enter
Sleep 2.5s

Type "hydronode sensors"
Sleep 300ms
Enter
Sleep 2.5s

Type "hydronode values $HN_DEMO_SENSOR/temperature --from 2d"
Sleep 300ms
Enter
Sleep 3.5s
