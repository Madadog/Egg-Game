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

// Strings directly printed with `print_raw()` must end with a
// null byte `\0`, while strings printed by the game's dialogue
// system must not.
pub static GAME_TITLE: &str = "super unfinished EGG GAME\0";
pub static GAME_TITLE_BLURB: &str = "v0.0.8\0";
pub static MENU_PLAY: &str = "Play\0";
pub static MENU_OPTIONS: &str = "Options\0";
pub static MENU_BACK: &str = "Back\0";
pub static OPTIONS_FONT_SIZE: &str = "Toggle Font Size\0";
pub static OPTIONS_FONT_FIXED: &str = "Toggle Fixed Font size\0";
pub static OPTIONS_RESET: &str = "Erase Data\0";
pub static OPTIONS_RESET_SURE: &str = "Erase Data (Press again to confirm)\0";
pub static OPTIONS_LOSE_DATA: &str = "You'll lose all data.\0";
pub static BEDROOM_MATTRESS: &str = "You can't get to sleep.";
pub static BEDROOM_TROLLEY: &str = "It's your baby bro's cot.";
pub static BEDROOM_CLOSET: &str = "Everything you have is in here.";
pub static BEDROOM_WINDOW: &str = "It's a beautiful day... \n            \n... Outside.";
pub static HOUSE_STAIRWELL_WINDOW: &str = "The glimmering gold sun ignites the hills, casting wild shadows over the landscape. You feel hopeful.";
pub static HOUSE_STAIRWELL_WINDOW2: &str = "By a twisted error of design, the view here lines up precisely with the neighbours' bathroom window.";
pub static HOUSE_STAIRWELL_PAINTING: &str = "A yellow circle hovers listlessly over a collection of purple lumps. You feel nothing in particular.";
pub static HOUSE_STAIRWELL_DOOR: &str = "You shouldn't go in there.";
pub static HOUSE_LIVING_ROOM_COUCH: &str = "He's busy.";
pub static HOUSE_LIVING_ROOM_TV_1: &str =
    "It's a cartoon of some sort. The protagonist is still charging up his main attack.";
pub static HOUSE_LIVING_ROOM_TV_2: &str = "Some sort of cartoon. While the hero was charging his attack, the villain took over the world.";
pub static HOUSE_LIVING_ROOM_TV_3: &str = "A long cartoon series. After taking over the world, the villain created global stability and happiness.";
pub static HOUSE_LIVING_ROOM_TV_4: &str = "Still the same cartoon. The hero finished his training arc, beat the villain and became the new ruler.";
pub static HOUSE_LIVING_ROOM_TV_5: &str = "This cartoon series refuses to end. The hero made some bad choices, now everyone wants the villain back.";
pub static HOUSE_LIVING_ROOM_TV_6: &str = "This series will last forever. The hero murdered the villain out of spite. His only enemy is the world.";
pub static HOUSE_LIVING_ROOM_WINDOW: &str =
    "You harbour some very strong feelings about gothic windows.\nNone of them are good.";
pub static HOUSE_KITCHEN_CUPBOARD: &str = "The cupboard is empty. Even the spiders have moved on.";
pub static HOUSE_KITCHEN_SINK: &str =
    "The unholy king of tacky windows. Words fail to convey your antipathy.";
pub static HOUSE_KITCHEN_MICROWAVE: &str = "Microwave, the oven of the future. It cooks everything; bread, mince meat, oxygen absorption packets...";
pub static UNKNOWN_1: &str = "The wispy ethers of your moral fibre hold this door shut.";
pub static UNKNOWN_2: &str = "... Quick, recast the \"Sleep\" shell on your opponent to keep it perma-stunned! Unfair and effective!";
pub static DEFAULT: &str = "You kids don't know what it's like. In my day, the gamedev didn't even assign me any interaction dialogue.";
pub static DOG_OBTAINED: &str = "Dog has joined the party!";
pub static DOG_RELINQUISHED: &str = "Dog has left the party.";
pub static HOUSE_BACKYARD_BASEMENT: &str = "A horrendous stench rises from the cellar.";
pub static HOUSE_BACKYARD_SHED: &str = "The shed door won't budge, but you could definitely open it with some of the POWER TOOLS inside... Oh wait.";
pub static HOUSE_BACKYARD_DOGHOUSE: &str = "SUBROUTINE \"DOG\" NOT FOUND. INITIATE DEFAULT SUBROUTINE: knock knock. whos there. no response. laughter.";
pub static HOUSE_BACKYARD_STORMDRAIN: &str = "Over the fence lies a deep canal. There is no way back up, not unless you can return from the dead.";
pub static HOUSE_BACKYARD_ANTHILL: &[&str] = &[
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
pub static INVENTORY_TITLE: &str = "INVENTORY\0";
pub static ITEM_FF_NAME: &str = "French Gry";
pub static ITEM_FF_DESC: &str =
    "A fried, thinly-sliced potato snack shaped like a guy. You feel a little bit ill.";
pub static ITEM_CHEGG_NAME: &str = "Mystery Egg";
pub static ITEM_CHEGG_DESC: &str = "There's something inside.";
pub static ITEM_LM_NAME: &str = "Little Man";
pub static ITEM_LM_DESC: &str = "\"... Hello? Somebody there? ... It's awful dark in here...\"";
pub static SM_COIN_RETURN: &str = "There's no money in the coin return slot.";
pub static SM_FRUIT_BASKET: &str = "They're not fresh.";
pub static SM_MAIN_WINDOW: &str = "It's yellow outside.";
pub static SM_FRIDGE_1: &str =
    "If you blow on the glass, it fogs up, revealing all the fingerprints left by prior employees.";
pub static SM_FRIDGE_2: &str =
    "A note on the front says \"Out of Order\", followed by a smiley face. A sickening contrast.";
pub static SM_VENDING_MACHINE: &str =
    "The blurb reads \"It's SodaTime!\". The machine is filled to the brim with cans of motor oil.";
pub static CONSTRUCTION_1: &str =
    "Looks like the creator didn't put too much effort into this part of the map.";
pub static CONSTRUCTION_2: &str = "Looks like it's still under construction.";
pub static EMERGENCY_EXIT: &str =
    "This is an emergency exit. It's not an emergency right now. Ergo, you cannot use the exit.";
pub static SM_HALL_SHELF: &str = "There's a single bottle of floor cleaner. And no mop.";
pub static SM_HALL_WINDOW: &str = "Looks like this window has recently been painted yellow.";
pub static EGG_1: &str = "It's floating.";
pub static SM_STOREROOM_SHELF: &str = "They're all out of Keratin Krunch.";
pub static SM_TITLE: &str = "S____MAR__T";
pub static INSTRUCTIONS: &str = "Instructions\n\n\nArrow keys: Move around.\n\n[Z]: Interact.\n\n[X]: Open inventory, Skip text.\n\n\nRemember to get regular sleep.\n\n\n\n    Press any button to continue.\0";
// pub static _: &str = "";
