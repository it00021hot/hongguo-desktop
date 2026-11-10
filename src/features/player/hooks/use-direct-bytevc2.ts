import { useCallback, useEffect, useEffectEvent, useRef } from 'react';
import { DirectBytevc2Player } from './direct-bytevc2';

export function useDirectBytevc2({
  source,
  videoRef,
  canvasRef,
  pendingSeekRef,
  onFirstFrame,
  onWaiting,
  onError,
}: {
  source: string | null;
  videoRef: React.RefObject<HTMLVideoElement | null>;
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  pendingSeekRef: React.RefObject<number>;
  onFirstFrame: (width: number, height: number) => void;
  onWaiting: (waiting: boolean) => void;
  onError: (error: unknown) => void;
}) {
  const playerRef = useRef<DirectBytevc2Player | null>(null);
  const firstFrame = useEffectEvent(onFirstFrame);
  const waiting = useEffectEvent(onWaiting);
  const failed = useEffectEvent(onError);

  useEffect(() => {
    const audio = videoRef.current;
    const canvas = canvasRef.current;
    if (!source || !audio || !canvas) return;
    let player: DirectBytevc2Player;
    try {
      player = new DirectBytevc2Player(
        source,
        audio,
        canvas,
        { firstFrame, waiting, failed },
        pendingSeekRef.current || audio.currentTime,
      );
    } catch (error) {
      failed(error);
      return;
    }
    playerRef.current = player;
    return () => {
      playerRef.current = null;
      player.dispose();
    };
  }, [source, videoRef, canvasRef, pendingSeekRef]);

  return useCallback(() => playerRef.current?.buffering === true, []);
}
