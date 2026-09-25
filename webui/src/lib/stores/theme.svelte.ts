export type ThemeMode = 'light' | 'dark' | 'system';

class ThemeStore {
  private mode = $state<ThemeMode>('system');
  private isDark = $state(false);

  constructor() {
    if (typeof window !== 'undefined') {
      const saved = localStorage.getItem('kanon-theme') as ThemeMode | null;
      if (saved) {
        this.mode = saved;
      }
      this.updateClass();

      window
        .matchMedia('(prefers-color-scheme: dark)')
        .addEventListener('change', () => {
          if (this.mode === 'system') {
            this.updateClass();
          }
        });
    }
  }

  public get currentMode(): ThemeMode {
    return this.mode;
  }

  public get dark(): boolean {
    return this.isDark;
  }

  public setMode(mode: ThemeMode) {
    this.mode = mode;
    localStorage.setItem('kanon-theme', mode);
    this.updateClass();
  }

  public toggle() {
    this.setMode(this.isDark ? 'light' : 'dark');
  }

  private updateClass() {
    const prefersDark = window.matchMedia(
      '(prefers-color-scheme: dark)',
    ).matches;
    this.isDark =
      this.mode === 'dark' || (this.mode === 'system' && prefersDark);
    document.documentElement.classList.toggle('dark', this.isDark);
  }
}

export const theme = new ThemeStore();
