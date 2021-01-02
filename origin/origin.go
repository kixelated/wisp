package main

import (
	"fmt"
	"log"
	"math/rand"
	"net/http"
	"strconv"
	"time"
)

func main() {
	http.HandleFunc("/video", handleVideo)
	log.Fatal(http.ListenAndServe(":9001", nil))
}

// Generates fake streaming video
func handleVideo(w http.ResponseWriter, r *http.Request) {
	err := serveVideo(w, r)
	if err != nil {
		http.Error(w, err.Error(), 500)
	}
}

func serveVideo(w http.ResponseWriter, r *http.Request) (err error) {
	ctx := r.Context()

	fps := 30
	duration := 2 * time.Second
	bitrate := 6 * 1000 * 1000 // 6Mb/s
	source := rand.NewSource(0)

	for key, params := range r.URL.Query() {
		switch key {
		case "seed":
			seed, err := strconv.Atoi(params[0])
			if err != nil {
				return fmt.Errorf("failed to parse seed: %w", err)
			}

			source = rand.NewSource(int64(seed))
		case "duration":
			durationMs, err := strconv.Atoi(params[0])
			if err != nil {
				return fmt.Errorf("failed to parse duration: %w", err)
			}

			if durationMs <= 0 {
				return fmt.Errorf("invalid duration")
			}

			duration = time.Duration(durationMs) * time.Millisecond
		case "fps":
			fps, err = strconv.Atoi(params[0])
			if err != nil {
				return fmt.Errorf("failed to parse fps: %w", err)
			}

			if fps <= 0 {
				return fmt.Errorf("invalid fps")
			}
		case "bitrate":
			bitrate, err = strconv.Atoi(params[0])
			if err != nil {
				return fmt.Errorf("failed to parse bitrate: %w", err)
			}

			if bitrate <= 0 {
				return fmt.Errorf("invalid bitrate")
			}
		}
	}

	flusher := w.(http.Flusher)

	random := rand.New(source)

	ticker := time.NewTicker(time.Second / time.Duration(fps))
	defer ticker.Stop()

	frameCount := int((time.Duration(fps) * duration) / time.Second)
	frameSize := bitrate / (8 * fps)

	frameBuffer := make([]byte, frameSize)

	for i := 0; i < frameCount; i += 1 {
		_, err = random.Read(frameBuffer)
		if err != nil {
			return nil
		}

		_, err = w.Write(frameBuffer)
		if err != nil {
			return nil
		}

		flusher.Flush()

		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-ticker.C:
		}
	}

	return nil
}
