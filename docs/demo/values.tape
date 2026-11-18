# Periods above a limit, and every value of one sensor
Output docs/demo/values.gif

Set Shell "bash"
Set FontSize 15
Set Width 1500
Set Height 760
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

Hide
Type "export HYDRONODE_PROFILE=${HN_DEMO_PROFILE:-demo} PS1='\n$ ' PATH=${HN_DEMO_BIN:+$HN_DEMO_BIN:}$PATH && clear"
Enter
Show

Type "hydronode values $HN_DEMO_SENSOR/temperature --above 28 --from 2d"
Sleep 300ms
Enter
Sleep 3s

Type "hydronode sensors show $HN_DEMO_SENSOR"
Sleep 300ms
Enter
Sleep 3.5s
