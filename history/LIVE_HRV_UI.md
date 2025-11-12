# Live HRV UI polish — November 12, 2025

- Added a “Live HRV snapshot” block (heart rate, RR mean/latest, RMSSD, LF/HF, beat count) in the ECG tab so presenters immediately see up-to-date metrics without hunting through the bottom panel.
- Surface per-run SQI tiles showing kurtosis, SNR/ RR CV along with progress bars and textual status, letting operators judge whether the current ECG stream is acceptable.
- The store now computes SQIs whenever ECG and RR data are fresh, and the UI only updates the tiles when both are present, keeping the worker/Store pipeline responsive.

- RR histogram: included a small distribution plot of the latest RR intervals so presenters can see dispersion/trends without exporting data.
