; ============================================================================
; 自定义 NSIS 安装钩子
; 目的：保留 productName="Knowledge Base"（让 CI 产物名/updater URL 全英文，
;       避免 GitHub Release Asset 中文字符 URL 编码导致更新失败），
;       同时让用户桌面/开始菜单的快捷方式显示中文"知识库"。
;
; 编码必须为 UTF-8 with BOM，否则 NSIS 编译器无法正确识别中文字面量。
; ============================================================================

!macro NSIS_HOOK_POSTINSTALL
  ; 删除 Tauri 默认创建的英文快捷方式
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"
  Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"

  ; 创建中文快捷方式（指向同一个 exe）
  CreateShortcut "$DESKTOP\知识库.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\知识库.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; 卸载时清理中文快捷方式
  Delete "$DESKTOP\知识库.lnk"
  Delete "$SMPROGRAMS\$AppStartMenuFolder\知识库.lnk"
!macroend