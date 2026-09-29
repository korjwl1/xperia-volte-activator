// shadcn-svelte 표준 유틸 + 타입 헬퍼 (컴포넌트 생성기가 $lib/utils에서 import)
export { cn } from "cn";

export type WithoutChild<T> = T extends { child?: unknown } ? WithoutChild<Omit<T, "child">> : T;
export type WithoutChildren<T> = T extends { children?: unknown } ? WithoutChildren<Omit<T, "children">> : T;
export type WithoutChildrenOrChild<T> = WithoutChildren<WithoutChild<T>>;
export type WithElementRef<T> = T & { ref?: HTMLElement | null };
