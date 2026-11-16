import { useEffect, useRef } from "react";
import { listModelJobs, subscribeModelJobs, type ModelJob } from "./foundry";

export function useModelJobStream(onJob: (job: ModelJob, hydrated: boolean) => void) {
  const latest = useRef(onJob);
  latest.current = onJob;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    listModelJobs()
      .then((existing) => {
        if (!disposed) {
          existing.forEach((job) => latest.current(job, true));
        }
      })
      .catch(() => undefined);

    subscribeModelJobs((job) => latest.current(job, false))
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}
