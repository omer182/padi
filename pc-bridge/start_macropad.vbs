' Launches the macropad power-mode supervisor fully hidden (no console window).
' Placed in the Windows Startup folder to auto-run at login.
Dim sh, base
Set sh = CreateObject("WScript.Shell")
base = "C:\Users\rio18\Projects\macropad-midi\pc-bridge\"
sh.CurrentDirectory = base
sh.Run """" & base & ".venv\Scripts\pythonw.exe"" """ & base & "supervisor.py""", 0, False
