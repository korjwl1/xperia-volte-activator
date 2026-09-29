<script lang="ts">
  interface Props {
    label: string;
    selected: number;
    total: number;
    onToggleAll: () => void;
  }

  let { label, selected, total, onToggleAll }: Props = $props();

  const allSelected = $derived(selected === total);
</script>

<div class="flex items-center gap-2 mb-2 px-1">
  <span class="h-1.5 w-1.5 rounded-full {allSelected ? 'bg-primary' : 'bg-muted-foreground/40'}"></span>
  <span class="text-xs font-semibold uppercase tracking-wide {allSelected ? 'text-foreground' : 'text-muted-foreground'}">
    {label}
  </span>
  <span class="text-[10px] text-muted-foreground">{selected}/{total}</span>
  <button
    class="ml-auto text-[11px] text-muted-foreground hover:text-foreground"
    onclick={onToggleAll}
  >
    {allSelected ? "전체 해제" : "전체 선택"}
  </button>
</div>
