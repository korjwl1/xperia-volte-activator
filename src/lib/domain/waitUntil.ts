/** 한 번에 하나의 질의만 실행한다. 질의가 끝나지 않아도 전체 제한 시간과 취소가 적용된다. */
export function waitUntil(check: () => Promise<boolean>, current: () => boolean, timeoutMs: number, everyMs = 2000): Promise<boolean> {
  return new Promise(resolve => {
    let finished = false;
    let next: ReturnType<typeof setTimeout> | undefined;
    const finish = (value: boolean) => {
      if (finished) return;
      finished = true;
      clearTimeout(deadline);
      clearInterval(cancel);
      if (next) clearTimeout(next);
      resolve(value);
    };
    const deadline = setTimeout(() => finish(false), timeoutMs);
    const cancel = setInterval(() => { if (!current()) finish(false); }, Math.min(everyMs, 100));
    const poll = async () => {
      if (finished || !current()) return finish(false);
      try {
        const ready = await check();
        if (finished) return;
        if (!current()) return finish(false);
        if (ready) return finish(true);
      } catch { /* 조회 실패는 다음 질의에서 재검사한다. */ }
      if (!finished) next = setTimeout(() => { void poll(); }, everyMs);
    };
    void poll();
  });
}
