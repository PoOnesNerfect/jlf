use owo_colors::{CssColors, DynColors, XtermColors};
use thiserror::Error;

pub fn parse_color(input: &str) -> Result<DynColors, ParseColorError> {
    // try parsing ansi colors or hex colors
    if let Ok(color) = input.parse() {
        return Ok(color);
    }

    let color = match input {
        // Css Colors
        "alice blue" => DynColors::Css(CssColors::AliceBlue),
        "antique white" => DynColors::Css(CssColors::AntiqueWhite),
        "aqua" => DynColors::Css(CssColors::Aqua),
        "aquamarine" => DynColors::Css(CssColors::Aquamarine),
        "azure" => DynColors::Css(CssColors::Azure),
        "beige" => DynColors::Css(CssColors::Beige),
        "bisque" => DynColors::Css(CssColors::Bisque),
        "black" => DynColors::Css(CssColors::Black),
        "blanched almond" => DynColors::Css(CssColors::BlanchedAlmond),
        "blue" => DynColors::Css(CssColors::Blue),
        "blue violet" => DynColors::Css(CssColors::BlueViolet),
        "brown" => DynColors::Css(CssColors::Brown),
        "burly wood" => DynColors::Css(CssColors::BurlyWood),
        "cadet blue" => DynColors::Css(CssColors::CadetBlue),
        "chartreuse" => DynColors::Css(CssColors::Chartreuse),
        "chocolate" => DynColors::Css(CssColors::Chocolate),
        "coral" => DynColors::Css(CssColors::Coral),
        "cornflower blue" => DynColors::Css(CssColors::CornflowerBlue),
        "cornsilk" => DynColors::Css(CssColors::Cornsilk),
        "crimson" => DynColors::Css(CssColors::Crimson),
        "dark blue" => DynColors::Css(CssColors::DarkBlue),
        "dark cyan" => DynColors::Css(CssColors::DarkCyan),
        "dark golden rod" => DynColors::Css(CssColors::DarkGoldenRod),
        "dark gray" => DynColors::Css(CssColors::DarkGray),
        "dark grey" => DynColors::Css(CssColors::DarkGrey),
        "dark green" => DynColors::Css(CssColors::DarkGreen),
        "dark khaki" => DynColors::Css(CssColors::DarkKhaki),
        "dark magenta" => DynColors::Css(CssColors::DarkMagenta),
        "dark olive green" => DynColors::Css(CssColors::DarkOliveGreen),
        "dark orange" => DynColors::Css(CssColors::DarkOrange),
        "dark orchid" => DynColors::Css(CssColors::DarkOrchid),
        "dark red" => DynColors::Css(CssColors::DarkRed),
        "dark salmon" => DynColors::Css(CssColors::DarkSalmon),
        "dark sea green" => DynColors::Css(CssColors::DarkSeaGreen),
        "dark slate blue" => DynColors::Css(CssColors::DarkSlateBlue),
        "dark slate gray" => DynColors::Css(CssColors::DarkSlateGray),
        "dark slate grey" => DynColors::Css(CssColors::DarkSlateGrey),
        "dark turquoise" => DynColors::Css(CssColors::DarkTurquoise),
        "dark violet" => DynColors::Css(CssColors::DarkViolet),
        "deep pink" => DynColors::Css(CssColors::DeepPink),
        "deep sky blue" => DynColors::Css(CssColors::DeepSkyBlue),
        "dim gray" => DynColors::Css(CssColors::DimGray),
        "dim grey" => DynColors::Css(CssColors::DimGrey),
        "dodger blue" => DynColors::Css(CssColors::DodgerBlue),
        "fire brick" => DynColors::Css(CssColors::FireBrick),
        "floral white" => DynColors::Css(CssColors::FloralWhite),
        "forest green" => DynColors::Css(CssColors::ForestGreen),
        "fuchsia" => DynColors::Css(CssColors::Fuchsia),
        "gainsboro" => DynColors::Css(CssColors::Gainsboro),
        "ghost white" => DynColors::Css(CssColors::GhostWhite),
        "gold" => DynColors::Css(CssColors::Gold),
        "golden rod" => DynColors::Css(CssColors::GoldenRod),
        "gray" => DynColors::Css(CssColors::Gray),
        "grey" => DynColors::Css(CssColors::Grey),
        "green" => DynColors::Css(CssColors::Green),
        "green yellow" => DynColors::Css(CssColors::GreenYellow),
        "honey dew" => DynColors::Css(CssColors::HoneyDew),
        "hot pink" => DynColors::Css(CssColors::HotPink),
        "indian red" => DynColors::Css(CssColors::IndianRed),
        "indigo" => DynColors::Css(CssColors::Indigo),
        "ivory" => DynColors::Css(CssColors::Ivory),
        "khaki" => DynColors::Css(CssColors::Khaki),
        "lavender" => DynColors::Css(CssColors::Lavender),
        "lavender blush" => DynColors::Css(CssColors::LavenderBlush),
        "lawn green" => DynColors::Css(CssColors::LawnGreen),
        "lemon chiffon" => DynColors::Css(CssColors::LemonChiffon),
        "light blue" => DynColors::Css(CssColors::LightBlue),
        "light coral" => DynColors::Css(CssColors::LightCoral),
        "light cyan" => DynColors::Css(CssColors::LightCyan),
        "light golden rod yellow" => {
            DynColors::Css(CssColors::LightGoldenRodYellow)
        }
        "light gray" => DynColors::Css(CssColors::LightGray),
        "light grey" => DynColors::Css(CssColors::LightGrey),
        "light green" => DynColors::Css(CssColors::LightGreen),
        "light pink" => DynColors::Css(CssColors::LightPink),
        "light salmon" => DynColors::Css(CssColors::LightSalmon),
        "light sea green" => DynColors::Css(CssColors::LightSeaGreen),
        "light sky blue" => DynColors::Css(CssColors::LightSkyBlue),
        "light slate gray" => DynColors::Css(CssColors::LightSlateGray),
        "light slate grey" => DynColors::Css(CssColors::LightSlateGrey),
        "light steel blue" => DynColors::Css(CssColors::LightSteelBlue),
        "light yellow" => DynColors::Css(CssColors::LightYellow),
        "lime" => DynColors::Css(CssColors::Lime),
        "lime green" => DynColors::Css(CssColors::LimeGreen),
        "linen" => DynColors::Css(CssColors::Linen),
        "magenta" => DynColors::Css(CssColors::Magenta),
        "maroon" => DynColors::Css(CssColors::Maroon),
        "medium aqua marine" => DynColors::Css(CssColors::MediumAquaMarine),
        "medium blue" => DynColors::Css(CssColors::MediumBlue),
        "medium orchid" => DynColors::Css(CssColors::MediumOrchid),
        "medium purple" => DynColors::Css(CssColors::MediumPurple),
        "medium sea green" => DynColors::Css(CssColors::MediumSeaGreen),
        "medium slate blue" => DynColors::Css(CssColors::MediumSlateBlue),
        "medium spring green" => DynColors::Css(CssColors::MediumSpringGreen),
        "medium turquoise" => DynColors::Css(CssColors::MediumTurquoise),
        "medium violet red" => DynColors::Css(CssColors::MediumVioletRed),
        "midnight blue" => DynColors::Css(CssColors::MidnightBlue),
        "mint cream" => DynColors::Css(CssColors::MintCream),
        "misty rose" => DynColors::Css(CssColors::MistyRose),
        "moccasin" => DynColors::Css(CssColors::Moccasin),
        "navajo white" => DynColors::Css(CssColors::NavajoWhite),
        "navy" => DynColors::Css(CssColors::Navy),
        "old lace" => DynColors::Css(CssColors::OldLace),
        "olive" => DynColors::Css(CssColors::Olive),
        "olive drab" => DynColors::Css(CssColors::OliveDrab),
        "orange" => DynColors::Css(CssColors::Orange),
        "orange red" => DynColors::Css(CssColors::OrangeRed),
        "orchid" => DynColors::Css(CssColors::Orchid),
        "pale golden rod" => DynColors::Css(CssColors::PaleGoldenRod),
        "pale green" => DynColors::Css(CssColors::PaleGreen),
        "pale turquoise" => DynColors::Css(CssColors::PaleTurquoise),
        "pale violet red" => DynColors::Css(CssColors::PaleVioletRed),
        "papaya whip" => DynColors::Css(CssColors::PapayaWhip),
        "peach puff" => DynColors::Css(CssColors::PeachPuff),
        "peru" => DynColors::Css(CssColors::Peru),
        "pink" => DynColors::Css(CssColors::Pink),
        "plum" => DynColors::Css(CssColors::Plum),
        "powder blue" => DynColors::Css(CssColors::PowderBlue),
        "purple" => DynColors::Css(CssColors::Purple),
        "rebecca purple" => DynColors::Css(CssColors::RebeccaPurple),
        "red" => DynColors::Css(CssColors::Red),
        "rosy brown" => DynColors::Css(CssColors::RosyBrown),
        "royal blue" => DynColors::Css(CssColors::RoyalBlue),
        "saddle brown" => DynColors::Css(CssColors::SaddleBrown),
        "salmon" => DynColors::Css(CssColors::Salmon),
        "sandy brown" => DynColors::Css(CssColors::SandyBrown),
        "sea green" => DynColors::Css(CssColors::SeaGreen),
        "sea shell" => DynColors::Css(CssColors::SeaShell),
        "sienna" => DynColors::Css(CssColors::Sienna),
        "silver" => DynColors::Css(CssColors::Silver),
        "sky blue" => DynColors::Css(CssColors::SkyBlue),
        "slate blue" => DynColors::Css(CssColors::SlateBlue),
        "slate gray" => DynColors::Css(CssColors::SlateGray),
        "slate grey" => DynColors::Css(CssColors::SlateGrey),
        "snow" => DynColors::Css(CssColors::Snow),
        "spring green" => DynColors::Css(CssColors::SpringGreen),
        "steel blue" => DynColors::Css(CssColors::SteelBlue),
        "tan" => DynColors::Css(CssColors::Tan),
        "teal" => DynColors::Css(CssColors::Teal),
        "thistle" => DynColors::Css(CssColors::Thistle),
        "tomato" => DynColors::Css(CssColors::Tomato),
        "turquoise" => DynColors::Css(CssColors::Turquoise),
        "violet" => DynColors::Css(CssColors::Violet),
        "wheat" => DynColors::Css(CssColors::Wheat),
        "white" => DynColors::Css(CssColors::White),
        "white smoke" => DynColors::Css(CssColors::WhiteSmoke),
        "yellow" => DynColors::Css(CssColors::Yellow),
        "yellow green" => DynColors::Css(CssColors::YellowGreen),

        // Xterm Colors
        "user black" => DynColors::Xterm(XtermColors::UserBlack),
        "user red" => DynColors::Xterm(XtermColors::UserRed),
        "user green" => DynColors::Xterm(XtermColors::UserGreen),
        "user yellow" => DynColors::Xterm(XtermColors::UserYellow),
        "user blue" => DynColors::Xterm(XtermColors::UserBlue),
        "user magenta" => DynColors::Xterm(XtermColors::UserMagenta),
        "user cyan" => DynColors::Xterm(XtermColors::UserCyan),
        "user white" => DynColors::Xterm(XtermColors::UserWhite),
        "user bright black" => DynColors::Xterm(XtermColors::UserBrightBlack),
        "user bright red" => DynColors::Xterm(XtermColors::UserBrightRed),
        "user bright green" => DynColors::Xterm(XtermColors::UserBrightGreen),
        "user bright yellow" => DynColors::Xterm(XtermColors::UserBrightYellow),
        "user bright blue" => DynColors::Xterm(XtermColors::UserBrightBlue),
        "user bright magenta" => {
            DynColors::Xterm(XtermColors::UserBrightMagenta)
        }
        "user bright cyan" => DynColors::Xterm(XtermColors::UserBrightCyan),
        "user bright white" => DynColors::Xterm(XtermColors::UserBrightWhite),
        "stratos blue" => DynColors::Xterm(XtermColors::StratosBlue),
        "navy blue" => DynColors::Xterm(XtermColors::NavyBlue),
        "camarone green" => DynColors::Xterm(XtermColors::CamaroneGreen),
        "blue stone" => DynColors::Xterm(XtermColors::BlueStone),
        "orient blue" => DynColors::Xterm(XtermColors::OrientBlue),
        "endeavour blue" => DynColors::Xterm(XtermColors::EndeavourBlue),
        "science blue" => DynColors::Xterm(XtermColors::ScienceBlue),
        "blue ribbon" => DynColors::Xterm(XtermColors::BlueRibbon),
        "japanese laurel" => DynColors::Xterm(XtermColors::JapaneseLaurel),
        "deep sea green" => DynColors::Xterm(XtermColors::DeepSeaGreen),
        "deep cerulean" => DynColors::Xterm(XtermColors::DeepCerulean),
        "lochmara blue" => DynColors::Xterm(XtermColors::LochmaraBlue),
        "azure radiance" => DynColors::Xterm(XtermColors::AzureRadiance),
        "light japanese laurel" => {
            DynColors::Xterm(XtermColors::LightJapaneseLaurel)
        }
        "jade" => DynColors::Xterm(XtermColors::Jade),
        "persian green" => DynColors::Xterm(XtermColors::PersianGreen),
        "bondi blue" => DynColors::Xterm(XtermColors::BondiBlue),
        "cerulean" => DynColors::Xterm(XtermColors::Cerulean),
        "light azure radiance" => {
            DynColors::Xterm(XtermColors::LightAzureRadiance)
        }
        "malachite" => DynColors::Xterm(XtermColors::Malachite),
        "caribbean green" => DynColors::Xterm(XtermColors::CaribbeanGreen),
        "light caribbean green" => {
            DynColors::Xterm(XtermColors::LightCaribbeanGreen)
        }
        "robin egg blue" => DynColors::Xterm(XtermColors::RobinEggBlue),
        "dark spring green" => DynColors::Xterm(XtermColors::DarkSpringGreen),
        "light spring green" => DynColors::Xterm(XtermColors::LightSpringGreen),
        "bright turquoise" => DynColors::Xterm(XtermColors::BrightTurquoise),
        "cyan" => DynColors::Xterm(XtermColors::Cyan),
        "rosewood" => DynColors::Xterm(XtermColors::Rosewood),
        "pompadour magenta" => DynColors::Xterm(XtermColors::PompadourMagenta),
        "pigment indigo" => DynColors::Xterm(XtermColors::PigmentIndigo),
        "dark purple" => DynColors::Xterm(XtermColors::DarkPurple),
        "electric indigo" => DynColors::Xterm(XtermColors::ElectricIndigo),
        "electric purple" => DynColors::Xterm(XtermColors::ElectricPurple),
        "verdun green" => DynColors::Xterm(XtermColors::VerdunGreen),
        "scorpion olive" => DynColors::Xterm(XtermColors::ScorpionOlive),
        "lilac" => DynColors::Xterm(XtermColors::Lilac),
        "scampi indigo" => DynColors::Xterm(XtermColors::ScampiIndigo),
        "dark cornflower blue" => {
            DynColors::Xterm(XtermColors::DarkCornflowerBlue)
        }
        "dark limeade" => DynColors::Xterm(XtermColors::DarkLimeade),
        "glade green" => DynColors::Xterm(XtermColors::GladeGreen),
        "juniper green" => DynColors::Xterm(XtermColors::JuniperGreen),
        "hippie blue" => DynColors::Xterm(XtermColors::HippieBlue),
        "havelock blue" => DynColors::Xterm(XtermColors::HavelockBlue),
        "dark malibu blue" => DynColors::Xterm(XtermColors::DarkMalibuBlue),
        "dark bright green" => DynColors::Xterm(XtermColors::DarkBrightGreen),
        "dark pastel green" => DynColors::Xterm(XtermColors::DarkPastelGreen),
        "pastel green" => DynColors::Xterm(XtermColors::PastelGreen),
        "downy teal" => DynColors::Xterm(XtermColors::DownyTeal),
        "viking" => DynColors::Xterm(XtermColors::Viking),
        "malibu blue" => DynColors::Xterm(XtermColors::MalibuBlue),
        "bright green" => DynColors::Xterm(XtermColors::BrightGreen),
        "dark screamin green" => {
            DynColors::Xterm(XtermColors::DarkScreaminGreen)
        }
        "screamin green" => DynColors::Xterm(XtermColors::ScreaminGreen),
        "dark aquamarine" => DynColors::Xterm(XtermColors::DarkAquamarine),
        "light aquamarine" => DynColors::Xterm(XtermColors::LightAquamarine),
        "dark fresh eggplant" => {
            DynColors::Xterm(XtermColors::DarkFreshEggplant)
        }
        "light fresh eggplant" => {
            DynColors::Xterm(XtermColors::LightFreshEggplant)
        }
        "electric violet" => DynColors::Xterm(XtermColors::ElectricViolet),
        "light electric violet" => {
            DynColors::Xterm(XtermColors::LightElectricViolet)
        }
        "copper rose" => DynColors::Xterm(XtermColors::CopperRose),
        "strikemaster purple" => {
            DynColors::Xterm(XtermColors::StrikemasterPurple)
        }
        "deluge purple" => DynColors::Xterm(XtermColors::DelugePurple),
        "dark medium purple" => DynColors::Xterm(XtermColors::DarkMediumPurple),
        "dark heliotrope purple" => {
            DynColors::Xterm(XtermColors::DarkHeliotropePurple)
        }
        "clay creek olive" => DynColors::Xterm(XtermColors::ClayCreekOlive),
        "wild blue yonder" => DynColors::Xterm(XtermColors::WildBlueYonder),
        "chetwode blue" => DynColors::Xterm(XtermColors::ChetwodeBlue),
        "light limeade" => DynColors::Xterm(XtermColors::LightLimeade),
        "chelsea cucumber" => DynColors::Xterm(XtermColors::ChelseaCucumber),
        "bay leaf" => DynColors::Xterm(XtermColors::BayLeaf),
        "gulf stream" => DynColors::Xterm(XtermColors::GulfStream),
        "polo blue" => DynColors::Xterm(XtermColors::PoloBlue),
        "light malibu blue" => DynColors::Xterm(XtermColors::LightMalibuBlue),
        "pistachio" => DynColors::Xterm(XtermColors::Pistachio),
        "light pastel green" => DynColors::Xterm(XtermColors::LightPastelGreen),
        "dark feijoa green" => DynColors::Xterm(XtermColors::DarkFeijoaGreen),
        "vista blue" => DynColors::Xterm(XtermColors::VistaBlue),
        "bermuda" => DynColors::Xterm(XtermColors::Bermuda),
        "dark anakiwa blue" => DynColors::Xterm(XtermColors::DarkAnakiwaBlue),
        "chartreuse green" => DynColors::Xterm(XtermColors::ChartreuseGreen),
        "light screamin green" => {
            DynColors::Xterm(XtermColors::LightScreaminGreen)
        }
        "dark mint green" => DynColors::Xterm(XtermColors::DarkMintGreen),
        "mint green" => DynColors::Xterm(XtermColors::MintGreen),
        "lighter aquamarine" => {
            DynColors::Xterm(XtermColors::LighterAquamarine)
        }
        "anakiwa blue" => DynColors::Xterm(XtermColors::AnakiwaBlue),
        "bright red" => DynColors::Xterm(XtermColors::BrightRed),
        "dark flirt" => DynColors::Xterm(XtermColors::DarkFlirt),
        "flirt" => DynColors::Xterm(XtermColors::Flirt),
        "light flirt" => DynColors::Xterm(XtermColors::LightFlirt),
        "bright electric violet" => {
            DynColors::Xterm(XtermColors::BrightElectricViolet)
        }
        "roseof sharon orange" => {
            DynColors::Xterm(XtermColors::RoseofSharonOrange)
        }
        "matrix pink" => DynColors::Xterm(XtermColors::MatrixPink),
        "tapestry pink" => DynColors::Xterm(XtermColors::TapestryPink),
        "fuchsia pink" => DynColors::Xterm(XtermColors::FuchsiaPink),
        "heliotrope" => DynColors::Xterm(XtermColors::Heliotrope),
        "pirate gold" => DynColors::Xterm(XtermColors::PirateGold),
        "muesli orange" => DynColors::Xterm(XtermColors::MuesliOrange),
        "pharlap pink" => DynColors::Xterm(XtermColors::PharlapPink),
        "bouquet" => DynColors::Xterm(XtermColors::Bouquet),
        "light heliotrope" => DynColors::Xterm(XtermColors::LightHeliotrope),
        "buddha gold" => DynColors::Xterm(XtermColors::BuddhaGold),
        "olive green" => DynColors::Xterm(XtermColors::OliveGreen),
        "hillary olive" => DynColors::Xterm(XtermColors::HillaryOlive),
        "silver chalice" => DynColors::Xterm(XtermColors::SilverChalice),
        "wistful lilac" => DynColors::Xterm(XtermColors::WistfulLilac),
        "melrose lilac" => DynColors::Xterm(XtermColors::MelroseLilac),
        "rio grande green" => DynColors::Xterm(XtermColors::RioGrandeGreen),
        "conifer green" => DynColors::Xterm(XtermColors::ConiferGreen),
        "feijoa" => DynColors::Xterm(XtermColors::Feijoa),
        "pixie green" => DynColors::Xterm(XtermColors::PixieGreen),
        "jungle mist" => DynColors::Xterm(XtermColors::JungleMist),
        "light anakiwa blue" => DynColors::Xterm(XtermColors::LightAnakiwaBlue),
        "light mint green" => DynColors::Xterm(XtermColors::LightMintGreen),
        "celadon" => DynColors::Xterm(XtermColors::Celadon),
        "aero blue" => DynColors::Xterm(XtermColors::AeroBlue),
        "french pass light blue" => {
            DynColors::Xterm(XtermColors::FrenchPassLightBlue)
        }
        "guardsman red" => DynColors::Xterm(XtermColors::GuardsmanRed),
        "razzmatazz cerise" => DynColors::Xterm(XtermColors::RazzmatazzCerise),
        "hollywood cerise" => DynColors::Xterm(XtermColors::HollywoodCerise),
        "dark purple pizzazz" => {
            DynColors::Xterm(XtermColors::DarkPurplePizzazz)
        }
        "brighter electric violet" => {
            DynColors::Xterm(XtermColors::BrighterElectricViolet)
        }
        "tenn orange" => DynColors::Xterm(XtermColors::TennOrange),
        "roman orange" => DynColors::Xterm(XtermColors::RomanOrange),
        "cranberry pink" => DynColors::Xterm(XtermColors::CranberryPink),
        "hopbush pink" => DynColors::Xterm(XtermColors::HopbushPink),
        "lighter heliotrope" => {
            DynColors::Xterm(XtermColors::LighterHeliotrope)
        }
        "mango tango" => DynColors::Xterm(XtermColors::MangoTango),
        "copperfield" => DynColors::Xterm(XtermColors::Copperfield),
        "sea pink" => DynColors::Xterm(XtermColors::SeaPink),
        "can can pink" => DynColors::Xterm(XtermColors::CanCanPink),
        "light orchid" => DynColors::Xterm(XtermColors::LightOrchid),
        "bright heliotrope" => DynColors::Xterm(XtermColors::BrightHeliotrope),
        "dark corn" => DynColors::Xterm(XtermColors::DarkCorn),
        "dark tacha orange" => DynColors::Xterm(XtermColors::DarkTachaOrange),
        "tan beige" => DynColors::Xterm(XtermColors::TanBeige),
        "clam shell" => DynColors::Xterm(XtermColors::ClamShell),
        "thistle pink" => DynColors::Xterm(XtermColors::ThistlePink),
        "mauve" => DynColors::Xterm(XtermColors::Mauve),
        "corn" => DynColors::Xterm(XtermColors::Corn),
        "tacha orange" => DynColors::Xterm(XtermColors::TachaOrange),
        "deco orange" => DynColors::Xterm(XtermColors::DecoOrange),
        "pale goldenrod" => DynColors::Xterm(XtermColors::PaleGoldenrod),
        "alto beige" => DynColors::Xterm(XtermColors::AltoBeige),
        "fog pink" => DynColors::Xterm(XtermColors::FogPink),
        "chartreuse yellow" => DynColors::Xterm(XtermColors::ChartreuseYellow),
        "canary" => DynColors::Xterm(XtermColors::Canary),
        "honeysuckle" => DynColors::Xterm(XtermColors::Honeysuckle),
        "reef pale yellow" => DynColors::Xterm(XtermColors::ReefPaleYellow),
        "snowy mint" => DynColors::Xterm(XtermColors::SnowyMint),
        "oyster bay" => DynColors::Xterm(XtermColors::OysterBay),
        "dark rose" => DynColors::Xterm(XtermColors::DarkRose),
        "rose" => DynColors::Xterm(XtermColors::Rose),
        "light hollywood cerise" => {
            DynColors::Xterm(XtermColors::LightHollywoodCerise)
        }
        "purple pizzazz" => DynColors::Xterm(XtermColors::PurplePizzazz),
        "blaze orange" => DynColors::Xterm(XtermColors::BlazeOrange),
        "bittersweet orange" => {
            DynColors::Xterm(XtermColors::BittersweetOrange)
        }
        "wild watermelon" => DynColors::Xterm(XtermColors::WildWatermelon),
        "dark hot pink" => DynColors::Xterm(XtermColors::DarkHotPink),
        "pink flamingo" => DynColors::Xterm(XtermColors::PinkFlamingo),
        "flush orange" => DynColors::Xterm(XtermColors::FlushOrange),
        "vivid tangerine" => DynColors::Xterm(XtermColors::VividTangerine),
        "pink salmon" => DynColors::Xterm(XtermColors::PinkSalmon),
        "dark lavender rose" => DynColors::Xterm(XtermColors::DarkLavenderRose),
        "blush pink" => DynColors::Xterm(XtermColors::BlushPink),
        "yellow sea" => DynColors::Xterm(XtermColors::YellowSea),
        "texas rose" => DynColors::Xterm(XtermColors::TexasRose),
        "tacao" => DynColors::Xterm(XtermColors::Tacao),
        "sundown" => DynColors::Xterm(XtermColors::Sundown),
        "cotton candy" => DynColors::Xterm(XtermColors::CottonCandy),
        "lavender rose" => DynColors::Xterm(XtermColors::LavenderRose),
        "dandelion" => DynColors::Xterm(XtermColors::Dandelion),
        "grandis caramel" => DynColors::Xterm(XtermColors::GrandisCaramel),
        "caramel" => DynColors::Xterm(XtermColors::Caramel),
        "cosmos salmon" => DynColors::Xterm(XtermColors::CosmosSalmon),
        "pink lace" => DynColors::Xterm(XtermColors::PinkLace),
        "laser lemon" => DynColors::Xterm(XtermColors::LaserLemon),
        "dolly yellow" => DynColors::Xterm(XtermColors::DollyYellow),
        "portafino yellow" => DynColors::Xterm(XtermColors::PortafinoYellow),
        "cumulus" => DynColors::Xterm(XtermColors::Cumulus),
        "dark cod gray" => DynColors::Xterm(XtermColors::DarkCodGray),
        "cod gray" => DynColors::Xterm(XtermColors::CodGray),
        "light cod gray" => DynColors::Xterm(XtermColors::LightCodGray),
        "dark mine shaft" => DynColors::Xterm(XtermColors::DarkMineShaft),
        "mine shaft" => DynColors::Xterm(XtermColors::MineShaft),
        "light mine shaft" => DynColors::Xterm(XtermColors::LightMineShaft),
        "dark tundora" => DynColors::Xterm(XtermColors::DarkTundora),
        "tundora" => DynColors::Xterm(XtermColors::Tundora),
        "scorpion gray" => DynColors::Xterm(XtermColors::ScorpionGray),
        "dark dove gray" => DynColors::Xterm(XtermColors::DarkDoveGray),
        "dove gray" => DynColors::Xterm(XtermColors::DoveGray),
        "boulder" => DynColors::Xterm(XtermColors::Boulder),
        "dusty gray" => DynColors::Xterm(XtermColors::DustyGray),
        "nobel gray" => DynColors::Xterm(XtermColors::NobelGray),
        "dark silver chalice" => {
            DynColors::Xterm(XtermColors::DarkSilverChalice)
        }
        "light silver chalice" => {
            DynColors::Xterm(XtermColors::LightSilverChalice)
        }
        "dark silver" => DynColors::Xterm(XtermColors::DarkSilver),
        "dark alto" => DynColors::Xterm(XtermColors::DarkAlto),
        "alto" => DynColors::Xterm(XtermColors::Alto),
        "mercury" => DynColors::Xterm(XtermColors::Mercury),
        "gallery gray" => DynColors::Xterm(XtermColors::GalleryGray),
        _ => {
            return Err(ParseColorError {
                input: input.to_string(),
            })
        }
    };

    Ok(color)
}

#[derive(Debug, Error)]
#[error("Failed to parse color: {input}")]
pub struct ParseColorError {
    pub input: String,
}
