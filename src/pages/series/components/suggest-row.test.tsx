import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import type { SuggestItem } from '@/service/schema';
import { SuggestRow } from './suggest-row';

const item: SuggestItem = {
  word: '本地联想',
  parts: [],
  seriesId: 'offline-series',
  vid: '',
  cover: '/player-frame.png',
  abstract: '全80集',
  bookTitle: '',
  bookCover: '',
  bookScore: '',
};

function render(cover: string) {
  return renderToStaticMarkup(
    <SuggestRow item={{ ...item, cover }} active={false} onHover={() => {}} onPick={() => {}} />,
  );
}

afterEach(() => vi.unstubAllGlobals());

describe('suggestion covers', () => {
  it('renders an already supported cover without requiring a conversion proxy', () => {
    expect(render(item.cover)).toContain('src="/player-frame.png"');
  });

  it('preserves the offline resource boundary when falling back to the original cover', () => {
    vi.stubGlobal('__HONGGUO_OFFLINE_QA', '1');
    expect(render('https://example.com/cover.jpg')).not.toContain('<img');
    expect(render('')).not.toContain('<img');
  });
});
