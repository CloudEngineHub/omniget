export const shellLayout = $state({ bottomInset: 0 });

export function reserveShellBottom(node: HTMLElement) {
  const update = () => { shellLayout.bottomInset = node.getBoundingClientRect().height; };
  const observer = new ResizeObserver(update);
  observer.observe(node);
  update();
  return {
    destroy() {
      observer.disconnect();
      shellLayout.bottomInset = 0;
    },
  };
}
