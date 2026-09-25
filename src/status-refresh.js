// A slow startup read must not overwrite a newer configuration or restore.
export function createStatusRefresh({ read, apply, onError, onPending }) {
  let revision = 0;
  let mutationActive = false;
  let pending = false;
  const setPending = value => {
    pending = value;
    onPending(value);
  };
  const invalidate = () => {
    revision += 1;
    setPending(false);
  };
  return {
    get pending() { return pending; },
    invalidate,
    setMutationActive(value) {
      mutationActive = value;
      invalidate();
    },
    async refresh({ allowDuringMutation = false } = {}) {
      if (mutationActive && !allowDuringMutation) return 'superseded';
      const ticket = ++revision;
      setPending(true);
      try {
        const result = await read();
        if (ticket !== revision) return 'superseded';
        apply(result);
        return 'applied';
      } catch (error) {
        if (ticket !== revision) return 'superseded';
        onError(error);
        return 'failed';
      } finally {
        if (ticket === revision) setPending(false);
      }
    },
  };
}
