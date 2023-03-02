// Copyright (c) 2023 Adam Godwin <evilspamalt/at/gmail.com>
//
// This file is part of Egg Game - https://github.com/Madadog/Egg-Game/
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program. If not, see <https://www.gnu.org/licenses/>.

use crate::{
    dialogue::TextContent::{self, *},
    portraits, sound,
};

// Strings directly printed with `print_raw()` must end with a
// null byte `\0`, while strings printed by the game's dialogue
// system must not.
pub const GAME_TITLE: &str = "super unfinished EGG GAME\0";
pub const GAME_TITLE_BLURB: &str = "version 0.0.14\0";
pub const MENU_PLAY: &str = "Play\0";
pub const MENU_OPTIONS: &str = "Options\0";
pub const MENU_BACK: &str = "Back\0";
pub const MENU_EXIT: &str = "Exit to Menu\0";
pub const MENU_DEBUG_CONTROLS: &[&str] = &[
    "Palette 1\0",
    "Palette 2\0",
    "Palette 3\0",
    "Remove CameraBounds\0",
    "Toggle Dog\0",
    "Add creature\0",
];
pub const OPTIONS_TITLE: &str = "super unfinished OPTIONS MENU\0";
pub const OPTIONS_FONT_SIZE: &str = "Toggle Font Size\0";
pub const OPTIONS_RESET: &str = "Erase Data\0";
pub const OPTIONS_RESET_SURE: &str = "Erase Data (Press again to confirm)\0";
pub const OPTIONS_LOSE_DATA: &str = "You'll lose all data.\0";
pub const BEDROOM_MATTRESS: &str = "You can't get to sleep.";
pub const BEDROOM_TROLLEY: &str = "It's your baby bro's cot.";
pub const BEDROOM_CLOSET: &str = "Everything you have is in here.";
pub const BEDROOM_WINDOW: &[TextContent] = &[
    Text("It's a beautiful day...\n\n"),
    Delayed("... Outside.", 30),
];
pub const THING: &[TextContent] = &[
    Text("This shouldn't be here..."),
    Pause,
    Portrait(Some(&portraits::Y_NORMAL.to())),
    AutoText("You got that right. It's so.... so.... ... crappily drawn!!! What the heck is this thing, anyway?!?!"),
    Pause,
    Portrait(None),
    AutoText("I have no idea... I don't remember drawing this sprite. I don't even know where it came from."),
    Pause,
    Portrait(Some(&portraits::Y_NORMAL.to())),
    AutoText("Sounds honest. On that note, when are you gonna hurry up and add the actual gameplay?! This whole walking sim thing is getting kinda old..."),
    Pause,
    Portrait(None),
    AutoText("Stop. You are hurting my feelings."),
    Pause,
    Portrait(Some(&portraits::Y_AWAY.to())),
    AutoText("Fiiiiiiiiiine. I'll just walk around this empty map forever I guess,"),
    Delayed(" without a single thing to do, and with no living things to interact with.", 10),
    Pause,
    Portrait(None),
    AutoText("What about that creature on the couch?"),
    Pause,
    Portrait(Some(&portraits::Y_LOOK.to())),
    AutoText("He"),
    Delayed(" LITERALLY", 20),
    Delayed(" doesn't qualify.", 10),
    Delayed(" Like, as a living thing.", 30),
    Pause,
    Portrait(None),
    AutoText("You've also got a dog... He's right there..."),
    Pause,
    Portrait(Some(&portraits::Y_NORMAL.to())),
    AutoText("I can't pet him. What good is a dog in a game if you can't pet it?"),
    Pause,
    Portrait(None),
    Sound(&sound::EQUIP_OBTAINED),
    AutoText("[GAMEDEV] took critical damage...! You won the battle!"),
    Text("Earned 0 Exp. Received:\n* Responsibility for your actions.\n* Nothing else in particular.")
];
pub const HOUSE_STAIRWELL_WINDOW: &str = "The glimmering gold sun ignites the hills, casting wild shadows over the landscape. You feel hopeful.";
pub const HOUSE_STAIRWELL_WINDOW2: &str = "By a twisted error of design, the view here lines up precisely with the neighbours' bathroom window.";
pub const HOUSE_STAIRWELL_PAINTING_INIT: &str = "It's not as good as the real thing.";
pub const HOUSE_STAIRWELL_PAINTING_AFTER: &str = "A yellow circle hovers listlessly over a collection of purple lumps. You feel nothing in particular.";
pub const HOUSE_STAIRWELL_DOOR: &str = "You shouldn't go in there.";
pub const HOUSE_LIVING_ROOM_COUCH: &str = "He's busy.";
pub const HOUSE_LIVING_ROOM_TV_1: &str =
    "It's a cartoon of some sort. The protagonist is still charging up his main attack.";
pub const HOUSE_LIVING_ROOM_TV_2: &str = "Some sort of cartoon. While the hero was charging his attack, the villain took over the world.";
pub const HOUSE_LIVING_ROOM_TV_3: &str = "A long cartoon series. After taking over the world, the villain created global stability and happiness.";
pub const HOUSE_LIVING_ROOM_TV_4: &str = "Still the same cartoon. The hero finished his training arc, beat the villain and became the new ruler.";
pub const HOUSE_LIVING_ROOM_TV_5: &str = "This cartoon series refuses to end. The hero made some bad choices, now everyone wants the villain back.";
pub const HOUSE_LIVING_ROOM_TV_6: &str = "This series will last forever. The hero murdered the villain out of spite. His only enemy is the world.";
pub const HOUSE_LIVING_ROOM_WINDOW: &[TextContent] = &[
    Text("You harbour some very strong feelings about gothic windows.\n"),
    Delayed("None of them are good.", 30),
];
pub const HOUSE_KITCHEN_CUPBOARD: &str = "The cupboard is empty. Even the spiders have moved on.";
pub const HOUSE_KITCHEN_SINK: &[TextContent] = &[
    Sound(&sound::ALERT_UP),
    Delayed("Found something down the drain...!\n", 0),
    Text("... You left it there."),
    Delayed("\n\nThis isn't an RPG, after all.", 30),
];
pub const HOUSE_KITCHEN_WINDOW: &str =
    "The unholy king of tacky windows. Words fail to convey your antipathy.";
pub const HOUSE_KITCHEN_MICROWAVE: &str = "Microwave, the oven of the future. It cooks everything; bread, mince meat, oxygen absorption packets...";
pub const UNKNOWN_1: &str = "The wispy ethers of your moral fibre hold this door shut.";
pub const UNKNOWN_2: &str = "... Quick, recast the \"Sleep\" shell on your opponent to keep it perma-stunned! Unfair and effective!";
pub const UNKNOWN_3: &str = "... Don't get your hopes up.";
pub const DEFAULT: &str = "You kids don't know what it's like. In my day, the gamedev didn't even assign me any interaction dialogue.";
pub const DOG_OBTAINED: &str = "Dog has joined the party!";
pub const DOG_RELINQUISHED: &str = "Dog has left the party.";
pub const HOUSE_BACKYARD_BASEMENT: &str = "A horrendous stench rises from the cellar.";
pub const HOUSE_BACKYARD_SHED: &str = "The shed door won't budge, but you could easily open it with some of the POWER TOOLS inside... Oh wait.";
pub const HOUSE_BACKYARD_SHED_WINDOW: &str =
    "You can't actually see anything through this window.";
pub const HOUSE_BACKYARD_NEIGHBOURS: &[&str] = &[
    "You don't know much about the neighbours.",
    "... The traffic makes it difficult to get to their house.",
];
pub const HOUSE_BACKYARD_DOGHOUSE: &str = "SUBROUTINE \"DOG\" NOT FOUND. INITIATE DEFAULT SUBROUTINE: knock knock. whos there. no response. laughter.";
pub const HOUSE_BACKYARD_STORMDRAIN: &str = "Over the fence lies a deep canal. There is no way back up, not unless you can return from the dead.";
pub const HOUSE_BACKYARD_ANTHILL: &[&str] = &[
    "bb_grin",
    "Hey sis, let's play \"stick our fingers in the anthill!\"",
    "y_disgust",
    "... Wow, uh,,,,,,",
    "y_judgy",
    "... Maybe, let's not????",
    "bb_joy",
    "Whoever keeps their finger the longest wins~~!!!",
    "y_joy",
    "You're on~!!!",
    "y_sober",
    "Ok, but seriously, how long have you been playing?",
    "bb_grin",
    "I've been here *all* morning...... Heh, it kinda stings, actually!",
    "y_worry",
    "... You're bleeding... and we don't have any more PlasterAids...",
    "bb_grin",
    "I'll be right as rain, don't you sweat, sister... Juuuust as soon as I get all these ants off me...",
    "y_worry",
    "... That is a LOT of ants...",
    "bb_worry",
    "They're in my socks!",
    "y_horror",
    "I'll get the hose.",
    "bb_horror",
    "Please hurry!!!",
];
pub const TOWN_TRAFFIC: &str = "They've been stuck like this for a while now.";
pub const TOWN_LAMPPOST: &str =
    "Strangely enough, this pole isn't casting a shadow. This will undoubtedly become relevant later.";
pub const TOWN_HOME_WINDOW: &str = "It's not as bad from the outside.";
pub const TOWN_WIDE: &[TextContent] = &[
    Text("T"),
    Delayed("h", 10),
    Delayed("i", 10),
    Delayed("s", 10),
    Delayed(" ", 10),
    Delayed("t", 10),
    Delayed("e", 10),
    Delayed("x", 10),
    Delayed("t", 10),
    Delayed(" ", 10),
    Delayed("i", 10),
    Delayed("s", 10),
    Delayed(" ", 10),
    Delayed("w", 10),
    Delayed("i", 10),
    Delayed("d", 10),
    Delayed("e", 10),
    Delayed("...", 10),
];
pub const INVENTORY_TITLE: &str = "INVENTORY";
pub const INVENTORY_ITEMS: &str = "Items";
pub const INVENTORY_SHELL: &str = "Shell";
pub const INVENTORY_OPTIONS: &str = "Options";
pub const INVENTORY_BACK: &str = "Back";
pub const ITEM_FF_NAME: &str = "French Gry";
pub const ITEM_FF_DESC: &str =
    "A fried, thinly-sliced potato snack shaped like a guy. You feel a little bit ill.";
pub const ITEM_CHEGG_NAME: &str = "Mystery Egg";
pub const ITEM_CHEGG_DESC: &str = "There's something inside.";
pub const ITEM_LM_NAME: &str = "Little Man";
pub const ITEM_LM_DESC: &str = "\"... Hello? Somebody there? ... It's awful dark in here...\"";
pub const SM_COIN_RETURN: &str = "There's no money in the coin return slot.";
pub const SM_FRUIT_BASKET: &str = "They're not fresh.";
pub const SM_MAIN_WINDOW: &str = "It's yellow outside.";
pub const SM_FRIDGE_1: &str =
    "If you blow on the glass, it fogs up, revealing all the fingerprints left by prior employees.";
pub const SM_FRIDGE_2: &str =
    "A note on the front says \"Out of Order\", followed by a smiley face. A sickening contrast.";
pub const SM_VENDING_MACHINE: &str =
    "The blurb reads \"It's SodaTime!\". The machine is filled to the brim with cans of motor oil.";
pub const CONSTRUCTION_1: &str =
    "Looks like the creator didn't put too much effort into this part of the map.";
pub const CONSTRUCTION_2: &str = "Looks like it's still under construction.";
pub const EMERGENCY_EXIT: &str =
    "This is an emergency exit. It's not an emergency right now. Ergo, you cannot use the exit.";
pub const SM_HALL_SHELF: &str = "There's a single bottle of floor cleaner. And no mop.";
pub const SM_HALL_WINDOW: &str = "Looks like this window has recently been painted yellow.";
pub const EGG_1: &str = "It's floating.";
pub const SM_STOREROOM_SHELF: &str = "They're all out of Keratin Krunch.";
pub const SM_TITLE: &str = "S____MAR__T";
pub const INSTRUCTIONS_TITLE: &str = "Instructions";
pub const INSTRUCTIONS: &str = "Arrow keys: Move around.\n\n[Z]: Interact.\n\n[X]: Open inventory, Skip text.\n\n\nRemember to get regular sleep.\n\n\n\n    Press any button to continue.\0";
// pub const _: &str = "";
