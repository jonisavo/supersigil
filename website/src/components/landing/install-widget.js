function tabForHint(hint) {
  if (typeof hint !== 'string') {
    return null;
  }

  const normalizedHint = hint.toLowerCase();

  if (normalizedHint.includes('mac') || normalizedHint.includes('darwin')) {
    return 'homebrew';
  }

  if (normalizedHint.includes('linux')) {
    return 'aur';
  }

  if (normalizedHint.includes('win')) {
    return 'cargo';
  }

  return null;
}

export function detectInstallTab(navigatorLike = globalThis.navigator) {
  const platformHints = [
    navigatorLike?.userAgentData?.platform,
    navigatorLike?.platform,
    navigatorLike?.userAgent,
  ];

  for (const hint of platformHints) {
    const tabId = tabForHint(hint);
    if (tabId) {
      return tabId;
    }
  }

  return 'cargo';
}

export function initInstallWidget(widget, navigatorLike = globalThis.navigator) {
  const tabs = widget.querySelectorAll('.install-tab');
  const panels = widget.querySelectorAll('.install-panel');

  function selectTab(tabId) {
    tabs.forEach((tab) => {
      const isActive = tab.dataset.tab === tabId;
      tab.classList.toggle('active', isActive);
      tab.setAttribute('aria-selected', String(isActive));
    });

    panels.forEach((panel) => {
      const isActive = panel.id.startsWith(`panel-${tabId}-`);
      panel.classList.toggle('active', isActive);
      panel.hidden = !isActive;
    });
  }

  tabs.forEach((tab) => {
    tab.addEventListener('click', () => {
      const tabId = tab.dataset.tab;
      if (tabId) {
        selectTab(tabId);
      }
    });
  });

  widget.querySelectorAll('.install-copy').forEach((button) => {
    button.addEventListener('click', async () => {
      const text = button.dataset.copy;
      if (!text) {
        return;
      }

      try {
        await navigator.clipboard.writeText(text);
        const iconCopy = button.querySelector('.icon-copy');
        const iconCheck = button.querySelector('.icon-check');

        if (iconCopy instanceof HTMLElement && iconCheck instanceof HTMLElement) {
          iconCopy.classList.add('hidden');
          iconCheck.classList.add('visible');
          setTimeout(() => {
            iconCopy.classList.remove('hidden');
            iconCheck.classList.remove('visible');
          }, 2000);
        }
      } catch (error) {
        console.warn('Clipboard API unavailable:', error);
      }
    });
  });

  selectTab(detectInstallTab(navigatorLike));
}
