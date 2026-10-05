<script lang="ts">
  import { Dialog } from "bits-ui";
  import type { Snippet } from "svelte";
  let { title, onClose, children, class: className = "fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6" }:
    { title: string; onClose: () => void; children: Snippet; class?: string } = $props();
</script>

<Dialog.Root open={true} onOpenChange={(open) => { if (!open) onClose(); }}>
  <Dialog.Portal>
    <Dialog.Content class={className} onInteractOutside={(event) => event.preventDefault()}
      onEscapeKeydown={(event) => { event.preventDefault(); onClose(); }}>
      <Dialog.Title class="sr-only">{title}</Dialog.Title>
      {@render children()}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
