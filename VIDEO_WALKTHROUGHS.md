# Video Walkthrough Implementation Plan

## Summary

Created comprehensive video walkthrough scripts based on **actual Kelora features**. All commands have been tested and verified to work.

## What Was Created

### 1. **Screenplay Documentation** (`docs/video-walkthroughs.md`)

Four complete video scripts:

- **Video 1**: 90-second hero demo (social media)
- **Video 2**: 3-4 minute realistic debugging journey (primary demo)
- **Video 3**: 5-7 minute power user tour (comprehensive)
- **Video 4**: 2-3 minute comparison vs alternatives

### 2. **Demo Files** (`examples/`)

- **`demo_api_errors_large.jsonl`** (128 events)
  - Shows realistic error spike pattern
  - Normal operation → gradual errors → 6-min gap → error burst → recovery
  - Perfect for demonstrating --levelmap, --mark-gaps, --metrics

### 3. **Demo Commands Reference** (`examples/DEMO_COMMANDS.md`)

- All commands from each video
- Terminal setup tips
- Recording instructions (asciinema)
- Troubleshooting guide
- One-liners for social media

## Verified Features Used

All scripts use **only real Kelora features**:

✅ `-j` / `-f <format>` - Format detection
✅ `--levels` - Level filtering
✅ `--filter` - Expression-based filtering
✅ `--exec` - Field transformations
✅ `--metrics` with `track_count()`, `track_sum()`, `track_unique()`
✅ `--window` with `pluck_as_nums()` and `percentile()`
✅ `-F levelmap` - Visual timeline
✅ `--mark-gaps <duration>` - Time gap detection
✅ `--span` with `--span-close` - Aggregation
✅ `--begin`, `--end` - Pipeline stages
✅ `-F json|csv|logfmt` - Output formats
✅ `--save-alias` / `-a` - Config aliases
✅ 150+ Rhai functions (parsing, transforming, etc.)

## Test Results

```bash
# Levelmap visualization - ✅ Works perfectly
$ kelora -j examples/demo_api_errors_large.jsonl -F levelmap
2024-07-17T11:50:00.000Z IIIIIIIIIWIIIIIIIIEIEIEIEIEIEIEIEIEIEEIEIEIEIEIEIEIEIEIEIEIEIEIEIEIEIEIEIEI
2024-07-17T12:03:14.000Z EIEIEIEIEIEIEIEIEIEIEIEIEEEEEEEEEEEEEEEEEEEEEIIIIIIII

# Metrics aggregation - ✅ Shows endpoint breakdown
$ kelora -j examples/demo_api_errors_large.jsonl --levels error --metrics --exec 'track_count(e.endpoint)'
Metrics:
  /api/data    = 40
  /api/export  = 15
  /api/auth    = 7

# Gap detection - ✅ Identifies 6m35s deployment gap
$ kelora -j examples/demo_api_errors_large.jsonl --filter 'e.status >= 500' --mark-gaps 5m
________________________________________ time gap: 0:06:35 _________________________________________
```

## Recommended Next Steps

### 1. Record Videos (Priority Order)

**Start with Video 2** (Realistic Debugging Journey):
- Most relatable story
- Shows progression from simple to advanced
- 3-4 minutes is perfect for attention span
- Can be cut to 90s for social media

**Then Video 1** (90-Second Hero):
- Extract highlights from Video 2
- Add faster pacing
- Perfect for Twitter/LinkedIn/Reddit

**Then Video 3** (Power User Tour):
- Comprehensive feature showcase
- For documentation and website
- Can film in segments and splice

**Later: Video 4** (Comparisons):
- For positioning and landing page
- Requires more editing (split screen)

### 2. Production Setup

**Terminal Recording:**
```bash
# Install asciinema
brew install asciinema  # macOS
apt install asciinema   # Linux

# Record
asciinema rec demo.cast

# Convert to GIF (for docs)
npm install -g @asciinema/agg
agg demo.cast demo.gif

# Or upload and embed
asciinema upload demo.cast
```

**Terminal Settings:**
- Resolution: 1920x1080 or 1440x900
- Font: JetBrains Mono 18pt or Fira Code 18pt
- Theme: High contrast (Solarized Dark, Nord, Dracula)
- Prompt: `export PS1='$ '` (minimal)
- Working directory: Repository root

**Screen Layout:**
- 80-100 columns wide
- Clear buffer before each command
- Type at moderate pace (can speed up in post)

### 3. Narration

**Voice Recording:**
- Use a decent USB mic (Blue Yeti, Rode NT-USB)
- Record in a quiet room
- Energetic but not hyperactive
- Emphasize key terms: "streaming", "programmable", "one command"

**Script Delivery:**
- Conversational, not robotic
- Pause 1-2 seconds after each sentence
- Match narration to terminal actions
- Don't read code - explain what it does

### 4. Editing & Post-Production

**Video Editing:**
- Add text overlays for key concepts
- Highlight important command parts
- Add subtle transitions (fade)
- Include end cards with CTAs
- Optional: Add subtle background music (keep low)

**Export Formats:**
- MP4 (1080p H.264) for YouTube, website
- GIF (optimized) for documentation, GitHub
- WebM for web embedding
- Multiple lengths: 90s, 3m, 7m from same footage

### 5. Distribution Plan

**Website (kelora.dev):**
- Hero demo on landing page (Video 2, 3-min cut)
- Full tour in tutorials section (Video 3)
- Comparison on "Why Kelora" page (Video 4)

**Social Media:**
- Twitter/X: 90s version with captions
- LinkedIn: 3-min version with professional context
- Reddit: 3-min version on r/rust, r/programming, r/commandline
- Hacker News: Link to blog post with embedded video

**YouTube:**
- Upload all versions
- Use chapters/timestamps
- Add links in description
- Create playlist: "Kelora Tutorials"

**GitHub:**
- GIF in README.md (above the fold)
- Link to full videos in "Examples" section
- Embed in docs/quickstart.md

## File Manifest

```
docs/video-walkthroughs.md          Complete screenplays (4 videos)
examples/demo_api_errors_large.jsonl   Demo dataset (128 events)
examples/DEMO_COMMANDS.md           Command reference & recording guide
VIDEO_WALKTHROUGHS.md               This summary (you're reading it)
```

## Quick Start for Recording

```bash
# 1. Test all commands work
cd /home/user/kelora
./target/release/kelora -j examples/demo_api_errors_large.jsonl -F levelmap

# 2. Set up terminal
export PS1='$ '
clear

# 3. Start recording
asciinema rec video2-debugging.cast

# 4. Follow screenplay from docs/video-walkthroughs.md
# Type commands slowly, pause between outputs

# 5. Stop recording
# Press Ctrl+D

# 6. Review
asciinema play video2-debugging.cast

# 7. Convert or upload
agg video2-debugging.cast video2-debugging.gif
# or
asciinema upload video2-debugging.cast
```

## Tips for Success

**Do:**
- Practice commands before recording
- Use `clear` between major sections
- Let output render completely before next command
- Add `| head -20` if output is too long
- Keep narration simple and clear

**Don't:**
- Rush through commands
- Skip the "why" (just showing "what")
- Use placeholder data that doesn't tell a story
- Forget to show the end result
- Make it too technical (balance accessibility)

## Launch Strategy

1. **Week 1**: Record Video 2 (debugging journey)
2. **Week 2**: Edit, add narration, create 90s cut
3. **Week 3**: Record Video 3 (power tour)
4. **Week 4**: Polish all videos, create distribution package
5. **Week 5**: Launch with blog post + videos + social media blitz

## Success Metrics

Track:
- Video completion rate (aim for >60%)
- Click-through to kelora.dev (aim for >5%)
- GitHub stars from video viewers
- Social media engagement (shares, comments)
- Documentation page views after launch

---

**Ready to record!** All commands tested and working. Files in place. Scripts ready. 🎬
