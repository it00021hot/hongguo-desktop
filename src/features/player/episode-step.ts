import type { Episode } from '@/service/schema';

type EpisodeReference = Pick<Episode, 'vid' | 'vidIndex'>;

type EpisodeStep =
  | { kind: 'unavailable' }
  | { kind: 'missing' }
  | { kind: 'first' }
  | { kind: 'last' }
  | { kind: 'episode'; vidIndex: number };

export function resolveEpisodeStep(
  episodes: readonly EpisodeReference[] | undefined,
  currentIndex: number,
  delta: number,
): EpisodeStep {
  const ordered = orderedEpisodes(episodes ?? []);
  if (ordered.length === 0) return { kind: 'unavailable' };
  const current = ordered.findIndex((episode) => episode.vidIndex === currentIndex);
  if (current < 0 || !Number.isInteger(delta) || delta === 0) return { kind: 'missing' };
  const next = current + delta;
  if (next < 0) return { kind: 'first' };
  const episode = ordered[next];
  return episode ? { kind: 'episode', vidIndex: episode.vidIndex } : { kind: 'last' };
}

export function firstEpisodeIndex(episodes: readonly EpisodeReference[]): number | null {
  return orderedEpisodes(episodes)[0]?.vidIndex ?? null;
}

function orderedEpisodes(episodes: readonly EpisodeReference[]): EpisodeReference[] {
  return episodes
    .filter((episode) => episode.vid !== '')
    .toSorted((a, b) => a.vidIndex - b.vidIndex);
}
