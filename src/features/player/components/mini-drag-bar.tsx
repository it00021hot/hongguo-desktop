/** 小屏模式的窗口拖拽条：一条 24px 顶带（纯展示，显隐条件由调用处给）。 */

/* 小屏的拖拽条：顶栏在小屏不渲染（第三方小屏是纯播放器），
   窗口拖动职责移到这条 24px 顶带。stopPropagation：拖拽残留
   的 click 不能触发「点画面暂停」。 */
export function MiniDragBar() {
  return (
    <div
      data-tauri-drag-region
      onClick={(e) => e.stopPropagation()}
      className="absolute inset-x-0 top-0 z-30 h-6"
    />
  );
}
