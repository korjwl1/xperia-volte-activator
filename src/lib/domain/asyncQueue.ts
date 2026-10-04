/** 호출 순서가 의미 있는 저장·보관 작업을 직렬화한다. 실패 후에도 다음 작업은 실행한다. */
export class AsyncQueue {
  private tail: Promise<unknown> = Promise.resolve();

  push<T>(work: () => Promise<T>): Promise<T> {
    const result = this.tail.then(work, work);
    this.tail = result.catch(() => undefined);
    return result;
  }
}
