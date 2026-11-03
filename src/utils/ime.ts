/** IME 组合中判定：Enter 提交类快捷键在组合中必须让行（否则选词确认被当「发送」吞掉）；keyCode===229 兜 Safari 旧版组合中不置 isComposing。 */
export function isImeComposing(e: { isComposing: boolean; keyCode: number }): boolean {
  return e.isComposing || e.keyCode === 229;
}
