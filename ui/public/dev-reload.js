(() => {
  let instance;
  let unavailable = false;
  const probe = async () => {
    try {
      const response = await fetch("/health/live", { cache: "no-store" });
      const current = response.headers.get("x-rill-instance");
      if (response.ok && ((instance && current && current !== instance) || unavailable)) {
        location.reload();
        return;
      }
      instance = current || instance;
      unavailable = false;
    } catch {
      unavailable = true;
    }
    setTimeout(probe, 750);
  };
  void probe();
})();
