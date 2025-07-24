use crate::prelude::*;

pub fn get_metadata_keys(key: &str) -> &'static [&'static str] {
    // Strip all possible prefixes in sequence
    let normalized_key = key
        .strip_prefix("USER_")
        .or_else(|| key.strip_prefix("ASWG_"))
        .or_else(|| key.strip_prefix("BEXT_"))
        .or_else(|| key.strip_prefix("STEINBERG_"))
        .or_else(|| key.strip_prefix("VORBIS_"))
        .or_else(|| key.strip_prefix("WV_"))
        .or_else(|| key.strip_prefix("TAG_"))
        .unwrap_or(key)
        .to_lowercase()
        .replace([' ', '_', '-', '.'], "");

    match normalized_key.as_str() {
        "catid" => &["USER_CATID", "ASWG_catId", "TAG_CatID"],
        "category" => &[
            "USER_CATEGORY",
            "ASWG_category",
            "STEINBERG_MediaCategoryPost",
            "TAG_Genre",
            "TAG_Category",
        ],
        "subcategory" => &[
            "USER_SUBCATEGORY",
            "ASWG_subCategory",
            "STEINBERG_MusicalCategory",
            "TAG_SubCategory",
        ],
        "categoryfull" => &["USER_CATEGORYFULL", "TAG_CategoryFull"],
        "usercategory" => &["USER_USERCATEGORY", "ASWG_userCategory", "TAG_UserCategory"],
        "vendorcategory" => &["USER_VENDORCATEGORY", "TAG_VendorCategory"],
        "fxname" => &["USER_FXNAME", "TAG_FXName"],
        "tracktitle" | "songtitle" => &[
            "USER_TRACKTITLE",
            "ASWG_songTitle",
            "STEINBERG_SmfSongName",
            "TAG_Title",
        ],
        "description" | "comment" => &[
            "BEXT_BWF_DESCRIPTION",
            "USER_DESCRIPTION",
            "STEINBERG_MediaComment",
            "TAG_Comment",
            "TAG_Description",
        ],
        "keywords" => &[
            "USER_KEYWORDS",
            "STEINBERG_MusicalInstrument",
            "TAG_Keywords",
        ],
        "manufacturer" | "originator" => &[
            "USER_MANUFACTURER",
            "TAG_Manufacturer",
            "TAG_Originator",
            "ASWG_originator",
            "STEINBERG_MediaLibraryManufacturerName",
            "BEXT_BWF_ORIGINATOR",
            "BEXT_BWF_ORIGINATOR_REFERENCE",
        ],
        "library" | "source" => &[
            "USER_LIBRARY",
            "USER_SOURCE",
            "ASWG_library",
            "STEINBERG_MediaLibrary",
            "TAG_Library",
        ],
        "designer" | "artist" => &[
            "USER_DESIGNER",
            "STEINBERG_AudioSoundEditor",
            "TAG_Artist",
            "TAG_Designer",
        ],
        "show" => &["USER_SHOW", "TAG_Show"],
        "recmedium" | "rec" | "recorder" => &["USER_RECMEDIUM", "TAG_RecMedium"],
        "microphone" | "mic" | "mictype" => &[
            "USER_MICROPHONE",
            "TAG_Microphone",
            "ASWG_micType",
            "STEINBERG_MediaRecordingMethod",
        ],
        "micperspective" | "mcperspective" => &["USER_MICPERSPECTIVE", "TAG_MicPerspective"],

        "location" => &[
            "USER_LOCATION",
            "TAG_Location",
            "STEINBERG_MediaRecordingLocation",
        ],

        "usercomments" | "userdata" => &["USER_USERCOMMENTS", "TAG_UserComments"],

        "releasedate" => &["USER_RELEASEDATE", "ASWG_releaseDate", "TAG_RETAIL_DATE"],

        "rating" => &["USER_RATING", "TAG_Rating", "STEINBERG_MediaTrackNumber"],

        "embedder" => &["USER_EMBEDDER", "TAG_Embedder", "BEXT_BWF_CODING_HISTORY"],
        _ => &[],
    }
}

// pub fn set_soundminer_metadata(key: &str, value: &str, map: &mut HashMap<String, String>) {
//     match key
//         .strip_prefix("USER_")
//         .unwrap_or(key)
//         .strip_prefix("ASWG_")
//         .unwrap_or(key)
//         .strip_prefix("BEXT_")
//         .unwrap_or(key)
//         .strip_prefix("STEINBERG_")
//         .unwrap_or(key)
//         .strip_prefix("VORBIS_")
//         .unwrap_or(key)
//         .strip_prefix("WV_")
//         .unwrap_or(key)
//         .strip_prefix("TAG_")
//         .unwrap_or(key)
//         .to_lowercase()
//         .replace([' ', '_', '-', '.'], "")
//         .as_str()
//     {
//         "catid" => { &[
//             "USER_CATID",
//             "ASWG_catId",
//             "TAG_CatID",
//        ]}
//         "category" => { &[
//             "USER_CATEGORY",
//             "ASWG_category",
//             "STEINBERG_MediaCategoryPost",
//             "TAG_Genre",
//             "TAG_Category",
//        ]}
//         "subcategory" => { &[
//             "USER_SUBCATEGORY",
//             "ASWG_subCategory",
//             "STEINBERG_MusicalCategory",
//             "TAG_SubCategory",
//        ]}
//         "categoryfull" => { &[
//             "USER_CATEGORYFULL",
//             "TAG_CategoryFull",
//        ]}
//         "usercategory" => { &[
//             "USER_USERCATEGORY",
//             "ASWG_userCategory",
//             "TAG_UserCategory",
//        ]}
//         "vendorcategory" => { &[
//             "USER_VENDORCATEGORY",
//             "TAG_VendorCategory",
//        ]}
//         "fxname" => { &[
//             "USER_FXNAME",
//             "TAG_FXName",
//        ]}
//         "tracktitle" | "songtitle" => { &[
//             "USER_TRACKTITLE",
//             "ASWG_songTitle",
//             "STEINBERG_SmfSongName",
//             "TAG_Title",
//        ]}
//         "description" | "comment" => { &[
//             "BEXT_BWF_DESCRIPTION",
//             "USER_DESCRIPTION",
//             "STEINBERG_MediaComment",
//             "TAG_Comment",
//             "TAG_Description",
//        ]}
//         "keywords" => { &[
//             "USER_KEYWORDS",
//             "STEINBERG_MusicalInstrument",
//             "TAG_Keywords",
//        ]}
//         "manufacturer" | "originator" => { &[
//             "USER_MANUFACTURER",
//             "TAG_Manufacturer",
//             "TAG_Originator",
//             "ASWG_originator",

//                 "STEINBERG_MediaLibraryManufacturerName".to_string(),
//                 value.to_string(),
//             );
//             "BEXT_BWF_ORIGINATOR",

//                 "BEXT_BWF_ORIGINATOR_REFERENCE".to_string(),
//                 value.to_string(),
//             );
//        ]}
//         "library" | "source" => { &[
//             "USER_LIBRARY",
//             "USER_SOURCE",
//             "ASWG_library",
//             "STEINBERG_MediaLibrary",
//             "TAG_Library",
//        ]}
//         "designer" | "artist" => { &[
//             "USER_DESIGNER",
//             "STEINBERG_AudioSoundEditor",
//             "TAG_Artist",
//             "TAG_Designer",
//        ]}
//         "show" => { &[
//             "USER_SHOW",
//             "TAG_Show",
//        ]}
//         "recmedium" | "rec" | "recorder" => { &[
//             "USER_RECMEDIUM",
//             "TAG_RecMedium",
//        ]}
//         "microphone" | "mic" | "mictype" => { &[
//             "USER_MICROPHONE",
//             "TAG_Microphone",
//             "ASWG_micType",

//                 "STEINBERG_MediaRecordingMethod".to_string(),
//                 value.to_string(),
//             );
//        ]}
//         "micperspective" | "mcperspective" => { &[
//             "USER_MICPERSPECTIVE",
//             "TAG_MicPerspective",
//        ]}

//         "location" => { &[
//             "USER_LOCATION",
//             "TAG_Location",

//                 "STEINBERG_MediaRecordingLocation".to_string(),
//                 value.to_string(),
//             );
//        ]}

//         "releasedate" => { &[
//             "USER_RELEASEDATE",
//             "ASWG_releaseDate",
//             "TAG_RETAIL_DATE",
//        ]}

//         "rating" => { &[
//             "USER_RATING",
//             "TAG_Rating",
//             "STEINBERG_MediaTrackNumber",
//        ]}

//         "embedder" => { &[
//             "USER_EMBEDDER",
//             "TAG_Embedder",
//             "BEXT_BWF_CODING_HISTORY",
//        ]}
//         _ => { &[}
//    ]}
// }
