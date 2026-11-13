import type { Episode, Series, WatchHistoryItem } from '@/service/schema';
import { resolveResumeEntry } from '@/utils/resume-entry';
import { firstEpisodeIndex } from './episode-step';

type HistoryTarget = Pick<WatchHistoryItem, 'seriesId' | 'vid' | 'vidIndex' | 'positionMs'>;
type EpisodeArchive = Pick<Series, 'seriesId'> & {
  episodes: readonly Pick<Episode, 'vid' | 'vidIndex'>[];
};

export function resolveHistoryTarget(
  history: HistoryTarget,
  archive: EpisodeArchive,
): { vidIndex: number; positionMs: number } | null {
  if (history.seriesId !== archive.seriesId) return null;
  const episode = archive.episodes.find((candidate) =>
    history.vid !== '' ? candidate.vid === history.vid : candidate.vidIndex === history.vidIndex,
  );
  if (!episode?.vid) return null;
  return { vidIndex: episode.vidIndex, positionMs: history.positionMs };
}

export function resolveResumeTarget(
  archive: EpisodeArchive,
  localProgress: Parameters<typeof resolveResumeEntry>[0],
  historyItem: (HistoryTarget & Pick<WatchHistoryItem, 'updatedAtMs'>) | null | undefined,
): { vidIndex: number; positionMs: number } | null {
  const entry = resolveResumeEntry(localProgress, historyItem);
  if (!entry) {
    const firstIndex = firstEpisodeIndex(archive.episodes);
    return firstIndex == null ? null : { vidIndex: firstIndex, positionMs: 0 };
  }
  if (entry.positionMs > 0 && historyItem) return resolveHistoryTarget(historyItem, archive);
  return archive.episodes.some((episode) => episode.vidIndex === entry.index && episode.vid !== '')
    ? { vidIndex: entry.index, positionMs: 0 }
    : null;
}
