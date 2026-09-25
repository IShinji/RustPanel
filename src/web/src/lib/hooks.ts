import { useEffect, useRef } from "react";

/**
 * 只在挂载时跑一次的副作用(典型:页面首次加载数据)。
 * 页面里的 `load` 每次渲染都是新函数,直接放进 deps 会让它随任意状态变化重跑;
 * 这里用 ref 拿最新的回调,deps 保持为空,语义就是「挂载时一次」。
 */
export function useMountEffect(effect: () => unknown) {
  const effectRef = useRef(effect);
  effectRef.current = effect;
  useEffect(() => {
    void effectRef.current();
  }, []);
}
