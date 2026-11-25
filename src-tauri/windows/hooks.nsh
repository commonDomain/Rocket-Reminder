!macro NSIS_HOOK_POSTINSTALL
  CreateShortcut "$DESKTOP\Rocket Reminder.lnk" "$INSTDIR\rocket-reminder.exe"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  IfFileExists "$INSTDIR\reminders.db" 0 done
  MessageBox MB_YESNO|MB_ICONQUESTION "是否删除提醒事项数据库？选择“否”将保留历史记录。" IDNO done
  Delete "$INSTDIR\reminders.db"
done:
!macroend
