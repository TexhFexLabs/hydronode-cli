# Summaries, dry periods and week-over-week compare
Output docs/demo/values.gif

Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 560
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

Hide
Type "export HYDRONODE_PROFILE=int && clear"
Enter
Show

Type "hn values $HN_DEMO_SENSOR/humidity --below 40 --from 7d"
Sleep 300ms
Enter
Sleep 3s

Type "hn values compare $HN_DEMO_SENSOR/temperature --shift 7d"
Sleep 300ms
Enter
Sleep 3.5s
