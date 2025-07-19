use crate::prelude::*;

pub fn set_soundminer_metadata(key: &str, value: &str, map: &mut HashMap<String, String>) {
    match key
        .strip_prefix("USER_")
        .unwrap_or(key)
        .strip_prefix("ASWG_")
        .unwrap_or(key)
        .strip_prefix("BEXT_")
        .unwrap_or(key)
        .strip_prefix("STEINBERG_")
        .unwrap_or(key)
        .strip_prefix("VORBIS_")
        .unwrap_or(key)
        .strip_prefix("WV_")
        .unwrap_or(key)
        .strip_prefix("TAG_")
        .unwrap_or(key)
        .to_lowercase()
        .replace([' ', '_', '-', '.'], "")
        .as_str()
    {
        "catid" => {
            map.insert("USER_CATID".to_string(), value.to_string());
            map.insert("ASWG_catId".to_string(), value.to_string());
            map.insert("WV_CatID".to_string(), value.to_string());
        }
        "category" => {
            map.insert("USER_CATEGORY".to_string(), value.to_string());
            map.insert("ASWG_category".to_string(), value.to_string());
            map.insert("STEINBERG_MediaCategoryPost".to_string(), value.to_string());
            map.insert("VORBIS_Genre".to_string(), value.to_string());
            map.insert("WV_Genre".to_string(), value.to_string());
            map.insert("WV_Category".to_string(), value.to_string());
        }
        "subcategory" => {
            map.insert("USER_SUBCATEGORY".to_string(), value.to_string());
            map.insert("ASWG_subCategory".to_string(), value.to_string());
            map.insert("STEINBERG_MusicalCategory".to_string(), value.to_string());
            map.insert("WV_SubCategory".to_string(), value.to_string());
        }
        "categoryfull" => {
            map.insert("USER_CATEGORYFULL".to_string(), value.to_string());
            map.insert("WV_CategoryFull".to_string(), value.to_string());
        }
        "usercategory" => {
            map.insert("USER_USERCATEGORY".to_string(), value.to_string());
            map.insert("ASWG_userCategory".to_string(), value.to_string());
            map.insert("WV_UserCategory".to_string(), value.to_string());
        }
        "vendorcategory" => {
            map.insert("USER_VENDORCATEGORY".to_string(), value.to_string());
            map.insert("WV_VendorCategory".to_string(), value.to_string());
        }
        "fxname" => {
            map.insert("USER_FXNAME".to_string(), value.to_string());
            map.insert("WV_FXName".to_string(), value.to_string());
        }
        "tracktitle" | "songtitle" => {
            map.insert("USER_TRACKTITLE".to_string(), value.to_string());
            map.insert("ASWG_songTitle".to_string(), value.to_string());
            map.insert("STEINBERG_SmfSongName".to_string(), value.to_string());
            map.insert("VORBIS_Title".to_string(), value.to_string());
            map.insert("WV_Title".to_string(), value.to_string());
        }
        "description" => {
            map.insert("BEXT_BWF_DESCRIPTION".to_string(), value.to_string());
            map.insert("USER_DESCRIPTION".to_string(), value.to_string());
            map.insert("STEINBERG_MediaComment".to_string(), value.to_string());
            map.insert("VORBIS_Description".to_string(), value.to_string());
            map.insert("WV_Comment".to_string(), value.to_string());
            map.insert("WV_Description".to_string(), value.to_string());
        }
        "keywords" => {
            map.insert("USER_KEYWORDS".to_string(), value.to_string());
            map.insert("STEINBERG_MusicalInstrument".to_string(), value.to_string());
            map.insert("WV_Keywords".to_string(), value.to_string());
        }
        "manufacturer" | "originator" => {
            map.insert("USER_MANUFACTURER".to_string(), value.to_string());
            map.insert("WV_Manufacturer".to_string(), value.to_string());
            map.insert("ASWG_originator".to_string(), value.to_string());
            map.insert(
                "STEINBERG_MediaLibraryManufacturerName".to_string(),
                value.to_string(),
            );
            map.insert("BEXT_BWF_ORIGINATOR".to_string(), value.to_string());
            map.insert(
                "BEXT_BWF_ORIGINATOR_REFERENCE".to_string(),
                value.to_string(),
            );
        }
        "library" | "source" => {
            map.insert("USER_LIBRARY".to_string(), value.to_string());
            map.insert("USER_SOURCE".to_string(), value.to_string());
            map.insert("ASWG_library".to_string(), value.to_string());
            map.insert("STEINBERG_MediaLibrary".to_string(), value.to_string());
            map.insert("WV_Library".to_string(), value.to_string());
        }
        "designer" | "artist" => {
            map.insert("USER_DESIGNER".to_string(), value.to_string());
            map.insert("STEINBERG_AudioSoundEditor".to_string(), value.to_string());
            map.insert("VORBIS_Artist".to_string(), value.to_string());
            map.insert("WV_Artist".to_string(), value.to_string());
            map.insert("WV_Designer".to_string(), value.to_string());
        }
        "show" => {
            map.insert("USER_SHOW".to_string(), value.to_string());
            map.insert("WV_Show".to_string(), value.to_string());
        }
        "recmedium" | "rec" | "recorder" => {
            map.insert("USER_RECMEDIUM".to_string(), value.to_string());
            map.insert("WV_RecMedium".to_string(), value.to_string());
        }
        "microphone" | "mic" | "mictype" => {
            map.insert("USER_MICROPHONE".to_string(), value.to_string());
            map.insert("WV_Microphone".to_string(), value.to_string());
            map.insert("ASWG_micType".to_string(), value.to_string());
            map.insert(
                "STEINBERG_MediaRecordingMethod".to_string(),
                value.to_string(),
            );
        }
        "micperspective" | "mcperspective" => {
            map.insert("USER_MICPERSPECTIVE".to_string(), value.to_string());
            map.insert("WV_MicPerspective".to_string(), value.to_string());
        }

        "location" => {
            map.insert("USER_LOCATION".to_string(), value.to_string());
            map.insert("WV_Location".to_string(), value.to_string());
            map.insert(
                "STEINBERG_MediaRecordingLocation".to_string(),
                value.to_string(),
            );
        }

        "releasedate" => {
            map.insert("USER_RELEASEDATE".to_string(), value.to_string());
            map.insert("ASWG_releaseDate".to_string(), value.to_string());
            map.insert("VORBIS_RETAIL_DATE".to_string(), value.to_string());
        }

        "rating" => {
            map.insert("USER_RATING".to_string(), value.to_string());
            map.insert("STEINBERG_MediaTrackNumber".to_string(), value.to_string());
        }

        "embedder" => {
            map.insert("USER_EMBEDDER".to_string(), value.to_string());
            map.insert("BEXT_BWF_CODING_HISTORY".to_string(), value.to_string());
        }
        _ => {}
    }
}
