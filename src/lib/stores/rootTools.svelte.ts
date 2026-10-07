/** Shared with the desktop close handler; I/O must settle before releasing protection. */
export const rootToolsState = $state({ busy: false, label: "" });
