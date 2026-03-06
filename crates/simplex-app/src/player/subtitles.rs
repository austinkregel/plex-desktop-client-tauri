//! Subtitle track management using pipeline's safe tag queries.

use super::pipeline::PipelineApi;

pub struct SubtitleManager;

impl SubtitleManager {
    pub fn list_tracks(pipeline: &impl PipelineApi) -> Vec<SubtitleInfo> {
        let count = pipeline.subtitle_track_count();
        let mut tracks = Vec::new();
        for i in 0..count {
            let language = pipeline.subtitle_language(i);
            let title = pipeline.subtitle_title(i);
            tracks.push(SubtitleInfo {
                index: i,
                language,
                title,
            });
        }
        tracks
    }

    pub fn select_track(pipeline: &impl PipelineApi, index: i32) {
        pipeline.set_subtitle_track(index);
    }

    pub fn disable(pipeline: &impl PipelineApi) {
        pipeline.set_subtitle_track(-1);
    }
}

pub struct SubtitleInfo {
    pub index: i32,
    pub language: Option<String>,
    pub title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::pipeline::mock::MockPipeline;

    #[test]
    fn test_list_tracks_empty() {
        let mock = MockPipeline::new();
        let tracks = SubtitleManager::list_tracks(&mock);
        assert!(tracks.is_empty());
    }

    #[test]
    fn test_list_tracks_single() {
        let mut mock = MockPipeline::new();
        mock.subtitle_count = 1;
        mock.subtitle_languages = vec![Some("eng".to_string())];
        mock.subtitle_titles = vec![Some("English".to_string())];

        let tracks = SubtitleManager::list_tracks(&mock);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].index, 0);
        assert_eq!(tracks[0].language, Some("eng".to_string()));
        assert_eq!(tracks[0].title, Some("English".to_string()));
    }

    #[test]
    fn test_list_tracks_multiple() {
        let mut mock = MockPipeline::new();
        mock.subtitle_count = 3;
        mock.subtitle_languages = vec![Some("eng".to_string()), Some("spa".to_string()), None];
        mock.subtitle_titles = vec![
            Some("English".to_string()),
            None,
            Some("Commentary".to_string()),
        ];

        let tracks = SubtitleManager::list_tracks(&mock);
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[2].language, None);
        assert_eq!(tracks[2].title, Some("Commentary".to_string()));
    }

    #[test]
    fn test_select_track() {
        let mock = MockPipeline::new();
        SubtitleManager::select_track(&mock, 2);
        assert_eq!(mock.selected_subtitle.get(), Some(2));
    }

    #[test]
    fn test_disable() {
        let mock = MockPipeline::new();
        SubtitleManager::disable(&mock);
        assert_eq!(mock.selected_subtitle.get(), Some(-1));
    }
}
