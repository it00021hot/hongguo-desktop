/** 加载反馈细条：舞台顶部 2px 滑动的 hg-loadbar（亮起条件由调用处裁决）。 */
export function Loadbar() {
  return (
    <div className="pointer-events-none absolute inset-x-0 top-0 z-40 h-0.5">
      <div className="hg-loadbar-track">
        <div className="bg-primary hg-loadbar" />
      </div>
    </div>
  );
}
