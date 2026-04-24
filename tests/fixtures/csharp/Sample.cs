using System;
using Acme.Shared;

namespace Acme.Sample;

interface IGreeter
{
    string Greet(string name);
}

class UserService : IGreeter
{
    private const int MinimumLength = 1;

    public string Name { get; set; } = "";

    public string Greet(string name)
    {
        return Format(name);
    }

    private string Format(string name)
    {
        Console.WriteLine(MinimumLength);
        return name.Trim();
    }
}
