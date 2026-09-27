using { /Fortnite.com/Devices }
using { /Verse.org/Simulation }

probe_device := class(creative_device):
    OnBegin<override>()<suspends>:void =
        Print("Ready")
