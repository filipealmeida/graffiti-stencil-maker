const PATHS: Record<string, string> = {
  image: 'M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15.5 9.5h.01',
  templates: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
  command: 'M9 6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3z',
  history: 'M3 12a9 9 0 1 0 3-6.7L3 8M3 3v5h5M12 7v5l3 2',
  undo: 'M9 14 4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3',
  redo: 'm15 14 5-5-5-5M20 9H10a6 6 0 0 0 0 12h3',
  folder: 'M3 6h6l2 2h10v11H3z',
  save: 'M5 3h12l4 4v14H5zM8 3v6h8V3M8 21v-7h8v7',
  new: 'M6 3h8l5 5v13H6zM14 3v5h5M12 12v6M9 15h6',
  rerun: 'M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7',
  report: 'M6 3h9l4 4v14H6zM9 12h7M9 16h7M9 8h3',
  perf: 'M4 20V10M10 20V4M16 20v-7M22 20H2',
  console: 'M3 5h18v14H3zM7 10l3 2-3 2M12 15h5',
}

/** A small line icon. It is decorative: the button around it carries the name and the tooltip. */
export default function Icon({ name }: { name: keyof typeof PATHS }) {
  return (
    <svg className="icon" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
      <path d={PATHS[name]} />
    </svg>
  )
}
